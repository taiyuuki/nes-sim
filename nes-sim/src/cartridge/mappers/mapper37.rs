use super::Mapper;
use super::mmc3::Mmc3Core;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_LEN: usize = 0x2000;
const CHR_BANK_LEN_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// "ZZ" multicart board (iNES mapper 037, Super Mario Bros. + Tetris +
/// Nintendo World Cup). An MMC3 core wrapped with a game-select register
/// at $6000-$7FFF that offsets the MMC3 PRG/CHR bank numbers.
pub(super) struct Mapper37 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    mirroring: Mirroring,
    ex_reg: u8,
    core: Mmc3Core,
}

impl Mapper37 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; crate::cartridge::CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            mirroring,
            ex_reg: 0,
            core: Mmc3Core::new(),
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        let len = match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len(),
            ChrMemory::Ram(chr_ram) => chr_ram.len(),
        };
        (len / CHR_BANK_LEN_1K).max(1)
    }

    fn prg_bank(&self, slot: usize) -> usize {
        let raw = self.core.raw_prg_bank_value(slot);
        let ex = self.ex_reg as usize;
        let bank = (ex << 2 & 0x10)
            | (if ex & 0x3 == 0x3 { 0x08 } else { 0x00 })
            | (raw & (ex << 1 | 0x7));
        bank % self.prg_bank_count()
    }

    fn chr_bank(&self, slot: usize) -> usize {
        let raw = self.core.effective_chr_bank_value(slot);
        let bank = ((self.ex_reg as usize) << 5 & 0x80) | usize::from(raw & 0x7F);
        bank % self.chr_bank_count_1k()
    }
}

impl Mapper for Mapper37 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_LEN;
                let index = self.prg_bank(slot) * PRG_BANK_LEN + ((addr as usize) & 0x1FFF);
                Some(self.prg_rom[index])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                self.ex_reg = data & 0x7;
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
        let slot = (addr as usize) / CHR_BANK_LEN_1K;
        let index = self.chr_bank(slot) * CHR_BANK_LEN_1K + ((addr as usize) & 0x03FF);
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[index]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[index]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = (addr as usize) / CHR_BANK_LEN_1K;
        let index = self.chr_bank(slot) * CHR_BANK_LEN_1K + ((addr as usize) & 0x03FF);
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[index] = data;
        }
        true
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
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_u8(self.ex_reg);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        self.core.save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.ex_reg = reader.read_u8()? & 0x7;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for mapper 37 save state",
                ));
            }
        }
        self.core.load_state(reader)?;
        Ok(())
    }
}
