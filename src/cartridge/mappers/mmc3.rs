use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_LEN: usize = 0x2000;
const CHR_BANK_LEN_1K: usize = 0x0400;
// MMC3 boards only clock the scanline counter after A12 stays low long enough to
// filter out short sprite/prefetch pulses.
const A12_LOW_FILTER_PPU_CYCLES: u64 = 10;
// Compatibility guard:
// Super Contra stage 1 boss exposed extra A12-qualified clocks inside a single
// scanline-sized window. Until the PPU/MMC3 interaction is modeled more
// cycle-accurately, suppress duplicate IRQ clocks that occur too soon after the
// previous accepted edge.
//
// TODO: Replace this spacing guard with a stricter model of MMC3-visible PPU bus
// activity so scanline IRQ qualification comes from hardware-accurate fetch
// timing instead of a conservative compatibility heuristic.
const IRQ_CLOCK_MIN_SPACING_PPU_CYCLES: u64 = 200;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mmc3 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    mirroring: Mirroring,
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
    last_irq_clock_cycle: Option<u64>,
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
            bank_select: 0,
            bank_registers: [0; 8],
            prg_ram_enabled: true,
            prg_ram_write_protect: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload_pending: false,
            irq_enabled: false,
            irq_line: false,
            last_a12: false,
            a12_fall_cycle: 0,
            last_irq_clock_cycle: None,
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

    fn prg_mode(&self) -> bool {
        (self.bank_select & 0x40) != 0
    }

    fn chr_inversion(&self) -> bool {
        (self.bank_select & 0x80) != 0
    }

    fn prg_bank_number(&self, slot: usize) -> usize {
        let bank_count = self.prg_bank_count();
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

    fn prg_rom_index(&self, addr: u16) -> usize {
        let slot = ((addr - 0x8000) as usize) / PRG_BANK_LEN;
        let bank = self.prg_bank_number(slot);
        bank * PRG_BANK_LEN + ((addr as usize) & 0x1FFF)
    }

    fn chr_bank_number(&self, slot: usize) -> usize {
        let bank_count = self.chr_bank_count_1k();
        let reg0 = (self.bank_registers[0] as usize & !1) % bank_count;
        let reg1 = (self.bank_registers[1] as usize & !1) % bank_count;
        let reg2 = (self.bank_registers[2] as usize) % bank_count;
        let reg3 = (self.bank_registers[3] as usize) % bank_count;
        let reg4 = (self.bank_registers[4] as usize) % bank_count;
        let reg5 = (self.bank_registers[5] as usize) % bank_count;

        let mapped = if self.chr_inversion() {
            match slot {
                0 => reg2,
                1 => reg3,
                2 => reg4,
                3 => reg5,
                4 => reg0,
                5 => reg0 + 1,
                6 => reg1,
                7 => reg1 + 1,
                _ => unreachable!(),
            }
        } else {
            match slot {
                0 => reg0,
                1 => reg0 + 1,
                2 => reg1,
                3 => reg1 + 1,
                4 => reg2,
                5 => reg3,
                6 => reg4,
                7 => reg5,
                _ => unreachable!(),
            }
        };
        mapped % bank_count
    }

    fn chr_index(&self, addr: u16) -> usize {
        let slot = (addr as usize) / CHR_BANK_LEN_1K;
        let bank = self.chr_bank_number(slot);
        bank * CHR_BANK_LEN_1K + ((addr as usize) & 0x03FF)
    }

    fn clock_irq_counter(&mut self, ppu_cycle: u64) {
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
        self.last_irq_clock_cycle = Some(ppu_cycle);

        if self.irq_enabled || self.irq_line || reload_pending {
            Self::trace_mmc3(format_args!(
                "a12-clock ppu={} counter:{}->{} latch={} reload_pending={} enabled={} irq_line={}",
                ppu_cycle,
                old_counter,
                self.irq_counter,
                self.irq_latch,
                reload_pending,
                self.irq_enabled,
                self.irq_line
            ));
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
                if self.prg_ram_enabled && !self.prg_ram_write_protect {
                    self.prg_ram[(addr - 0x6000) as usize] = data;
                }
                true
            }
            0x8000..=0x9FFF => {
                if (addr & 1) == 0 {
                    self.bank_select = data;
                    Self::trace_mmc3_verbose(format_args!(
                        "cpu-write addr={:04X} bank_select={:02X} prg_mode={} chr_inversion={}",
                        addr,
                        data,
                        (data & 0x40) != 0,
                        (data & 0x80) != 0
                    ));
                } else {
                    let index = (self.bank_select & 0x07) as usize;
                    self.bank_registers[index] = data;
                    Self::trace_mmc3_verbose(format_args!(
                        "cpu-write addr={:04X} bank_reg[{}]={:02X}",
                        addr, index, data
                    ));
                }
                true
            }
            0xA000..=0xBFFF => {
                if (addr & 1) == 0 {
                    if !matches!(self.mirroring, Mirroring::FourScreen) {
                        self.mirroring = if (data & 0x01) == 0 {
                            Mirroring::Vertical
                        } else {
                            Mirroring::Horizontal
                        };
                    }
                    Self::trace_mmc3(format_args!(
                        "cpu-write addr={:04X} mirroring={:?}",
                        addr, self.mirroring
                    ));
                } else {
                    self.prg_ram_enabled = (data & 0x80) != 0;
                    self.prg_ram_write_protect = (data & 0x40) != 0;
                    Self::trace_mmc3_verbose(format_args!(
                        "cpu-write addr={:04X} prg_ram_enabled={} write_protect={}",
                        addr, self.prg_ram_enabled, self.prg_ram_write_protect
                    ));
                }
                true
            }
            0xC000..=0xDFFF => {
                if (addr & 1) == 0 {
                    self.irq_latch = data;
                    Self::trace_mmc3(format_args!(
                        "cpu-write addr={:04X} irq_latch={:02X}",
                        addr, data
                    ));
                } else {
                    self.irq_reload_pending = true;
                    Self::trace_mmc3(format_args!(
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
                    Self::trace_mmc3(format_args!(
                        "cpu-write addr={:04X} irq_enabled=0 irq_line=0",
                        addr
                    ));
                } else {
                    self.irq_enabled = true;
                    Self::trace_mmc3(format_args!("cpu-write addr={:04X} irq_enabled=1", addr));
                }
                true
            }
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
        let a12 = (addr & 0x1000) != 0;
        if !a12 && self.last_a12 {
            self.a12_fall_cycle = ppu_cycle;
        } else if a12 && !self.last_a12 {
            let low_span = ppu_cycle.saturating_sub(self.a12_fall_cycle);
            let clock_spacing_ok = self.last_irq_clock_cycle.is_none_or(|last| {
                ppu_cycle.saturating_sub(last) >= IRQ_CLOCK_MIN_SPACING_PPU_CYCLES
            });
            if low_span >= A12_LOW_FILTER_PPU_CYCLES && clock_spacing_ok {
                self.clock_irq_counter(ppu_cycle);
            } else {
                Self::trace_mmc3_verbose(format_args!(
                    "a12-ignore ppu={} low_span={} spacing_ok={} addr={:04X} counter={} latch={} reload_pending={} enabled={}",
                    ppu_cycle,
                    low_span,
                    clock_spacing_ok,
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

    fn irq_line(&self) -> bool {
        self.irq_line
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
        writer.write_u8(match self.mirroring {
            Mirroring::Horizontal => 0,
            Mirroring::Vertical => 1,
            Mirroring::FourScreen => 2,
            Mirroring::SPAGE0 => 3,
            Mirroring::SPAGE1 => 4,
        });
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
        writer.write_bool(self.last_irq_clock_cycle.is_some());
        writer.write_u64(self.last_irq_clock_cycle.unwrap_or_default());
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
        self.mirroring = match reader.read_u8()? {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::FourScreen,
            3 => Mirroring::SPAGE0,
            4 => Mirroring::SPAGE1,
            _ => return Err(SaveStateError::InvalidData("invalid MMC3 mirroring value")),
        };
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
        self.last_irq_clock_cycle = if reader.read_bool()? {
            Some(reader.read_u64()?)
        } else {
            let _ = reader.read_u64()?;
            None
        };
        Ok(())
    }
}
