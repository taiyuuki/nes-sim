use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_LEN: usize = 0x2000;
const CHR_BANK_LEN_1K: usize = 0x0400;
// MMC3 boards only clock the scanline counter after A12 stays low long enough to
// filter out short sprite/prefetch pulses.
const A12_LOW_FILTER_PPU_CYCLES: u64 = 10;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

// Shared MMC3 register state and IRQ/A12 logic. Future MMC3-derived boards can
// reuse this core while supplying their own final PRG/CHR mapping.
pub(super) struct Mmc3Core {
    bank_select: u8,
    bank_registers: [u8; 8],
    prg_ram_enabled: bool,
    prg_ram_write_protect: bool,
    irq_latch: u8,
    irq_counter: u8,
    irq_reload_pending: bool,
    irq_enabled: bool,
    irq_line: bool,
    last_a12: bool,
    a12_fall_cycle: u64,
}

impl Mmc3Core {
    pub(super) fn new() -> Self {
        Self {
            bank_select: 0,
            bank_registers: Self::default_bank_registers(),
            prg_ram_enabled: true,
            prg_ram_write_protect: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload_pending: false,
            irq_enabled: false,
            irq_line: false,
            last_a12: false,
            a12_fall_cycle: 0,
        }
    }

    fn default_bank_registers() -> [u8; 8] {
        // MMC3 powers up with a linear CHR view and the first two switchable 8 KiB
        // PRG banks visible before the game programs its own mapping.
        [0, 2, 4, 5, 6, 7, 0, 1]
    }

    pub(super) fn prg_bank_number(&self, bank_count: usize, slot: usize) -> usize {
        let last = bank_count - 1;
        let second_last = bank_count - 2;
        let bank6 = (self.bank_registers[6] as usize) % bank_count;
        let bank7 = (self.bank_registers[7] as usize) % bank_count;

        match (self.prg_mode(), slot) {
            (false, 0) => bank6,
            (false, 1) => bank7,
            (false, 2) => second_last,
            (false, 3) => last,
            (true, 0) => second_last,
            (true, 1) => bank7,
            (true, 2) => bank6,
            (true, 3) => last,
            _ => unreachable!(),
        }
    }

    pub(super) fn chr_bank_number(&self, bank_count: usize, slot: usize) -> usize {
        usize::from(self.effective_chr_bank_value(slot)) % bank_count
    }

    pub(super) fn prg_ram_enabled(&self) -> bool {
        self.prg_ram_enabled
    }

    pub(super) fn prg_ram_write_protect(&self) -> bool {
        self.prg_ram_write_protect
    }

    pub(super) fn write_register(
        &mut self,
        addr: u16,
        data: u8,
        mirroring: Option<&mut Mirroring>,
    ) -> bool {
        match addr {
            0x8000..=0x9FFF => {
                if (addr & 1) == 0 {
                    self.bank_select = data;
                    trace_mmc3_verbose(format_args!(
                        "cpu-write addr={:04X} bank_select={:02X} prg_mode={} chr_inversion={}",
                        addr,
                        data,
                        (data & 0x40) != 0,
                        (data & 0x80) != 0
                    ));
                } else {
                    let index = (self.bank_select & 0x07) as usize;
                    self.bank_registers[index] = data;
                    trace_mmc3_verbose(format_args!(
                        "cpu-write addr={:04X} bank_reg[{}]={:02X}",
                        addr, index, data
                    ));
                }
                true
            }
            0xA000..=0xBFFF => {
                if (addr & 1) == 0 {
                    if let Some(mirroring) = mirroring {
                        if !matches!(mirroring, Mirroring::FourScreen) {
                            *mirroring = if (data & 0x01) == 0 {
                                Mirroring::Vertical
                            } else {
                                Mirroring::Horizontal
                            };
                        }
                        trace_mmc3(format_args!(
                            "cpu-write addr={:04X} mirroring={:?}",
                            addr, mirroring
                        ));
                    }
                } else {
                    self.prg_ram_enabled = (data & 0x80) != 0;
                    self.prg_ram_write_protect = (data & 0x40) != 0;
                    trace_mmc3_verbose(format_args!(
                        "cpu-write addr={:04X} prg_ram_enabled={} write_protect={}",
                        addr, self.prg_ram_enabled, self.prg_ram_write_protect
                    ));
                }
                true
            }
            0xC000..=0xDFFF => {
                if (addr & 1) == 0 {
                    self.irq_latch = data;
                    trace_mmc3(format_args!(
                        "cpu-write addr={:04X} irq_latch={:02X}",
                        addr, data
                    ));
                } else {
                    self.irq_reload_pending = true;
                    trace_mmc3(format_args!(
                        "cpu-write addr={:04X} irq_reload_pending=1",
                        addr
                    ));
                }
                true
            }
            0xE000..=0xFFFF => {
                if (addr & 1) == 0 {
                    self.irq_enabled = false;
                    self.irq_line = false;
                    trace_mmc3(format_args!(
                        "cpu-write addr={:04X} irq_enabled=0 irq_line=0",
                        addr
                    ));
                } else {
                    self.irq_enabled = true;
                    trace_mmc3(format_args!("cpu-write addr={:04X} irq_enabled=1", addr));
                }
                true
            }
            _ => false,
        }
    }

    pub(super) fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        let a12 = (addr & 0x1000) != 0;
        if !a12 && self.last_a12 {
            self.a12_fall_cycle = ppu_cycle;
        } else if a12 && !self.last_a12 {
            let low_span = ppu_cycle.saturating_sub(self.a12_fall_cycle);
            if low_span >= A12_LOW_FILTER_PPU_CYCLES {
                self.clock_irq_counter(ppu_cycle, addr, low_span);
            } else {
                trace_mmc3_verbose(format_args!(
                    "a12-ignore ppu={} low_span={} addr={:04X} counter={} latch={} reload_pending={} enabled={}",
                    ppu_cycle,
                    low_span,
                    addr,
                    self.irq_counter,
                    self.irq_latch,
                    self.irq_reload_pending,
                    self.irq_enabled
                ));
            }
        }
        self.last_a12 = a12;
    }

    pub(super) fn irq_line(&self) -> bool {
        self.irq_line
    }

    pub(super) fn effective_chr_bank_value(&self, slot: usize) -> u8 {
        let reg0 = self.bank_registers[0] & !1;
        let reg1 = self.bank_registers[1] & !1;
        let reg2 = self.bank_registers[2];
        let reg3 = self.bank_registers[3];
        let reg4 = self.bank_registers[4];
        let reg5 = self.bank_registers[5];

        if self.chr_inversion() {
            match slot {
                0 => reg2,
                1 => reg3,
                2 => reg4,
                3 => reg5,
                4 => reg0,
                5 => reg0.wrapping_add(1),
                6 => reg1,
                7 => reg1.wrapping_add(1),
                _ => unreachable!(),
            }
        } else {
            match slot {
                0 => reg0,
                1 => reg0.wrapping_add(1),
                2 => reg1,
                3 => reg1.wrapping_add(1),
                4 => reg2,
                5 => reg3,
                6 => reg4,
                7 => reg5,
                _ => unreachable!(),
            }
        }
    }

    pub(super) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.bank_select);
        writer.write_bytes(&self.bank_registers);
        writer.write_bool(self.prg_ram_enabled);
        writer.write_bool(self.prg_ram_write_protect);
        writer.write_u8(self.irq_latch);
        writer.write_u8(self.irq_counter);
        writer.write_bool(self.irq_reload_pending);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_line);
        writer.write_bool(self.last_a12);
        writer.write_u64(self.a12_fall_cycle);
    }

    pub(super) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.bank_select = reader.read_u8()?;
        reader.read_bytes_into(&mut self.bank_registers)?;
        self.prg_ram_enabled = reader.read_bool()?;
        self.prg_ram_write_protect = reader.read_bool()?;
        self.irq_latch = reader.read_u8()?;
        self.irq_counter = reader.read_u8()?;
        self.irq_reload_pending = reader.read_bool()?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_line = reader.read_bool()?;
        self.last_a12 = reader.read_bool()?;
        self.a12_fall_cycle = reader.read_u64()?;
        Ok(())
    }

    fn prg_mode(&self) -> bool {
        (self.bank_select & 0x40) != 0
    }

    fn chr_inversion(&self) -> bool {
        (self.bank_select & 0x80) != 0
    }

    fn clock_irq_counter(&mut self, ppu_cycle: u64, addr: u16, low_span: u64) {
        let old_counter = self.irq_counter;
        let reload_pending = self.irq_reload_pending;
        if self.irq_counter == 0 || self.irq_reload_pending {
            self.irq_counter = self.irq_latch;
            self.irq_reload_pending = false;
        } else {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
        }

        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_line = true;
        }

        if self.irq_enabled || self.irq_line || reload_pending {
            trace_mmc3_verbose(format_args!(
                "a12-clock ppu={} addr={:04X} low_span={} counter:{}->{} latch={} reload_pending={} enabled={} irq_line={}",
                ppu_cycle,
                addr,
                low_span,
                old_counter,
                self.irq_counter,
                self.irq_latch,
                reload_pending,
                self.irq_enabled,
                self.irq_line
            ));
        }

        if self.irq_line {
            trace_mmc3(format_args!(
                "irq-hit ppu={} addr={:04X} low_span={} counter:{}->{} latch={}",
                ppu_cycle, addr, low_span, old_counter, self.irq_counter, self.irq_latch
            ));
        }
    }
}

pub(super) struct Mmc3 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    mirroring: Mirroring,
    core: Mmc3Core,
}

impl Mmc3 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr,
            mirroring,
            core: Mmc3Core::new(),
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_LEN
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len() / CHR_BANK_LEN_1K,
            ChrMemory::Ram(chr_ram) => chr_ram.len() / CHR_BANK_LEN_1K,
        }
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let slot = ((addr - 0x8000) as usize) / PRG_BANK_LEN;
        let bank = self.core.prg_bank_number(self.prg_bank_count(), slot);
        bank * PRG_BANK_LEN + ((addr as usize) & 0x1FFF)
    }

    fn chr_index(&self, addr: u16) -> usize {
        let slot = (addr as usize) / CHR_BANK_LEN_1K;
        let bank = self.core.chr_bank_number(self.chr_bank_count_1k(), slot);
        bank * CHR_BANK_LEN_1K + ((addr as usize) & 0x03FF)
    }
}

impl Mapper for Mmc3 {
    fn mapper_id(&self) -> u16 {
        4
    }

    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                if self.core.prg_ram_enabled() && !self.core.prg_ram_write_protect() {
                    self.prg_ram[(addr - 0x6000) as usize] = data;
                }
                true
            }
            0x8000..=0xFFFF => self
                .core
                .write_register(addr, data, Some(&mut self.mirroring)),
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let index = self.chr_index(addr);
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[index]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[index]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let index = self.chr_index(addr);
        match &mut self.chr {
            ChrMemory::Ram(chr_ram) => {
                chr_ram[index] = data;
                true
            }
            ChrMemory::Rom(_) => true,
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        self.core.check_a12(addr, ppu_cycle);
    }

    fn irq_line(&self) -> bool {
        self.core.irq_line()
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        writer.write_u8(encode_mirroring(self.mirroring));
        self.core.save_state(writer);
        writer.write_bool(false);
        writer.write_u64(0);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for MMC3 save state",
                ));
            }
        }
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.core.load_state(reader)?;
        let _ = reader.read_bool()?;
        let _ = reader.read_u64()?;
        Ok(())
    }
}

fn encode_mirroring(mirroring: Mirroring) -> u8 {
    match mirroring {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::FourScreen => 2,
        Mirroring::SPAGE0 => 3,
        Mirroring::SPAGE1 => 4,
    }
}

fn decode_mirroring(encoded: u8) -> Result<Mirroring, SaveStateError> {
    match encoded {
        0 => Ok(Mirroring::Horizontal),
        1 => Ok(Mirroring::Vertical),
        2 => Ok(Mirroring::FourScreen),
        3 => Ok(Mirroring::SPAGE0),
        4 => Ok(Mirroring::SPAGE1),
        _ => Err(SaveStateError::InvalidData("invalid MMC3 mirroring value")),
    }
}

#[cfg(test)]
fn trace_mmc3(_args: std::fmt::Arguments<'_>) {}

#[cfg(test)]
fn trace_mmc3_verbose(_args: std::fmt::Arguments<'_>) {}

#[cfg(not(test))]
fn trace_mmc3(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("NES_TRACE_MMC3").is_some() {
        eprintln!("{args}");
    }
}

#[cfg(not(test))]
fn trace_mmc3_verbose(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("NES_TRACE_MMC3_VERBOSE").is_some() {
        eprintln!("{args}");
    }
}
