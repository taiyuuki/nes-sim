use super::Mapper;
use super::mmc3::Mmc3Core;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K_LEN: usize = 0x2000;
const PRG_BANK_32K_LEN: usize = 0x8000;
const CHR_BANK_LEN_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// BMC SuperHiK 4-in-1 (iNES mapper 049). MMC3 core whose PRG/CHR banking
/// is offset by a multicart register at $6000-$7FFF; the register only
/// responds while PRG-RAM is enabled via $A001 bit 7.
pub(super) struct Mapper49 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    mirroring: Mirroring,
    ex_reg: u8,
    core: Mmc3Core,
}

impl Mapper49 {
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

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K_LEN).max(1)
    }

    fn prg_bank_count_32k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_32K_LEN).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        let len = match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len(),
            ChrMemory::Ram(chr_ram) => chr_ram.len(),
        };
        (len / CHR_BANK_LEN_1K).max(1)
    }

    fn prg_index(&self, addr: u16) -> usize {
        if self.ex_reg & 0x1 != 0 {
            let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K_LEN;
            let raw = self.core.raw_prg_bank_value(slot);
            let bank = ((self.ex_reg as usize) >> 2 & 0x30) | (raw & 0x0F);
            (bank % self.prg_bank_count_8k()) * PRG_BANK_8K_LEN + ((addr as usize) & 0x1FFF)
        } else {
            let bank = (self.ex_reg as usize) >> 4 & 0x3;
            (bank % self.prg_bank_count_32k()) * PRG_BANK_32K_LEN + ((addr as usize) - 0x8000)
        }
    }

    fn chr_bank(&self, slot: usize) -> usize {
        let raw = self.core.effective_chr_bank_value(slot);
        let bank = ((self.ex_reg as usize) << 1 & 0x180) | usize::from(raw & 0x7F);
        bank % self.chr_bank_count_1k()
    }
}

impl Mapper for Mapper49 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                if self.core.prg_ram_enabled() {
                    self.ex_reg = data;
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
        self.ex_reg = reader.read_u8()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for mapper 49 save state",
                ));
            }
        }
        self.core.load_state(reader)?;
        Ok(())
    }
}
