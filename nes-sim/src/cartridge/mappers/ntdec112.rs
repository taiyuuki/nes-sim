use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// NTDEC board (iNES mapper 112, used by Chinese releases such as San Guo
/// Zhi - Chi Bi Zhi Zhan). Two 8 KiB PRG banks are switchable and the CHR
/// layout mirrors the MMC3's, with 2 KiB pairs in one half.
pub(super) struct Ntdec112 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    which_bank: u8,
    prg_banks: [u8; 2],
    chr_regs: [u8; 6],
    mirroring: Mirroring,
}

impl Ntdec112 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr,
            which_bank: 0,
            prg_banks: [0, 1],
            chr_regs: [0; 6],
            mirroring,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_1K).max(1),
        }
    }

    fn chr_bank_number(&self, slot: usize) -> usize {
        let bank = match slot {
            0 | 1 => (self.chr_regs[0] & 0xFE) + slot as u8,
            2 | 3 => (self.chr_regs[1] & 0xFE) + (slot as u8 - 2),
            _ => self.chr_regs[slot - 2],
        };
        (bank as usize) % self.chr_bank_count_1k()
    }
}

impl Mapper for Ntdec112 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xBFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K;
                let bank = self.prg_banks[slot] as usize % self.prg_bank_count();
                Some(self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)])
            }
            0xC000..=0xFFFF => {
                let last = self.prg_rom.len() - (0x10000 - 0xC000);
                Some(self.prg_rom[last + (addr as usize - 0xC000)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = data;
                true
            }
            0x8000..=0x8FFF => {
                self.which_bank = data & 0x07;
                true
            }
            0xA000..=0xAFFF => {
                match self.which_bank {
                    0 => self.prg_banks[0] = data,
                    1 => self.prg_banks[1] = data,
                    index @ 2..=7 => self.chr_regs[(index - 2) as usize] = data,
                    _ => {}
                }
                true
            }
            0xE000..=0xEFFF => {
                self.mirroring = if (data & 0x01) != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let bank = self.chr_bank_number(addr as usize / CHR_BANK_1K);
        let index = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[index]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[index]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let bank = self.chr_bank_number(addr as usize / CHR_BANK_1K);
        let index = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[index] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
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
        writer.write_u8(self.which_bank);
        writer.write_bytes(&self.prg_banks);
        writer.write_bytes(&self.chr_regs);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for NTDEC 112 save state",
                ));
            }
        }
        self.which_bank = reader.read_u8()?;
        reader.read_bytes_into(&mut self.prg_banks)?;
        reader.read_bytes_into(&mut self.chr_regs)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
