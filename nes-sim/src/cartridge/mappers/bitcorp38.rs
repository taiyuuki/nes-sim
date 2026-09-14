use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;

/// Bit Corp board (iNES mapper 038, Crime Busters). A GNROM-style register
/// with the PRG and CHR bank fields swapped.
pub(super) struct BitCorp38 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl BitCorp38 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        }
    }
}

impl Mapper for BitCorp38 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let offset = (self.prg_bank as usize) * PRG_BANK_32K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.prg_bank = data & 0x03;
                self.chr_bank = (data >> 2) & 0x03;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) || self.chr_rom.is_empty() {
            return None;
        }
        let offset = (self.chr_bank as usize) * CHR_BANK_8K + addr as usize;
        Some(self.chr_rom[offset % self.chr_rom.len()])
    }

    fn ppu_write(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.prg_bank);
        writer.write_u8(self.chr_bank);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank = reader.read_u8()?;
        self.chr_bank = reader.read_u8()?;
        Ok(())
    }
}
