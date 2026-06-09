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

pub(super) struct Jf19 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    prg_bank: usize,
    chr_bank: usize,
    mirroring: Mirroring,
}

impl Jf19 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_16K
    }
}

impl Mapper for Jf19 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let offset = (addr as usize - 0x8000) % self.prg_rom.len();
                Some(self.prg_rom[offset])
            }
            0xC000..=0xFFFF => {
                let bank = self.prg_bank % self.prg_bank_count_16k();
                let offset = bank * PRG_BANK_16K + (addr as usize - 0xC000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                if (data & 0x80) != 0 {
                    self.prg_bank = (data & 0x0F) as usize;
                }
                if (data & 0x40) != 0 {
                    self.chr_bank = (data & 0x0F) as usize;
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
        let offset = self.chr_bank * CHR_BANK_8K + addr as usize;
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let offset = self.chr_bank * CHR_BANK_8K + addr as usize;
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            let len = chr_ram.len();
            chr_ram[offset % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.prg_bank as u8);
        writer.write_u8(self.chr_bank as u8);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank = reader.read_u8()? as usize;
        self.chr_bank = reader.read_u8()? as usize;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
