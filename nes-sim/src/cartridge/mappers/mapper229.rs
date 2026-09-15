use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// BMC 31-in-1 (iNES mapper 229). The write address latches the PRG 16 KiB
/// bank (mirrored into both halves, with banks 0/1 forced when the low
/// select bits are clear), the CHR 8 KiB bank and the mirroring bit.
pub(super) struct Mapper229 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    bank_low: usize,
    bank_high: usize,
    chr_bank: usize,
    mirroring: Mirroring,
}

impl Mapper229 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; crate::cartridge::CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            bank_low: 0,
            bank_high: 1,
            chr_bank: 0,
            mirroring,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn chr_bank_count(&self) -> usize {
        let len = match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len(),
            ChrMemory::Ram(chr_ram) => chr_ram.len(),
        };
        (len / CHR_BANK_8K).max(1)
    }
}

impl Mapper for Mapper229 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.bank_low % self.prg_bank_count();
                Some(self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0x8000)])
            }
            0xC000..=0xFFFF => {
                let bank = self.bank_high % self.prg_bank_count();
                Some(self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0xC000)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, _data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                let bank = addr as usize;
                if bank & 0x1E != 0 {
                    self.bank_low = bank & 0x1F;
                    self.bank_high = bank & 0x1F;
                } else {
                    self.bank_low = 0;
                    self.bank_high = 1;
                }
                self.chr_bank = bank;
                self.mirroring = if bank & 0x20 != 0 {
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
        let offset = (self.chr_bank % self.chr_bank_count()) * CHR_BANK_8K + addr as usize;
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let offset = (self.chr_bank % self.chr_bank_count()) * CHR_BANK_8K + addr as usize;
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[offset] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.bank_low as u8);
        writer.write_u8(self.bank_high as u8);
        writer.write_u16(self.chr_bank as u16);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.bank_low = reader.read_u8()? as usize;
        self.bank_high = reader.read_u8()? as usize;
        self.chr_bank = reader.read_u16()? as usize;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
