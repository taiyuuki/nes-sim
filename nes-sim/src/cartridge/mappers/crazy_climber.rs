use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;

/// Crazy Climber board (iNES mapper 180). The first 16 KiB PRG bank is
/// hardwired at $8000 and the switchable bank lives at $C000.
pub(super) struct CrazyClimber {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_bank: u8,
    mirroring: Mirroring,
}

impl CrazyClimber {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            mirroring,
        }
    }
}

impl Mapper for CrazyClimber {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => Some(self.prg_rom[addr as usize - 0x8000]),
            0xC000..=0xFFFF => {
                let bank = (self.prg_bank & 0x07) as usize;
                let offset = bank * PRG_BANK_16K + (addr as usize - 0xC000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.prg_bank = data & 0x07;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) || self.chr_rom.is_empty() {
            return None;
        }
        Some(self.chr_rom[addr as usize % self.chr_rom.len()])
    }

    fn ppu_write(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.prg_bank);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank = reader.read_u8()?;
        Ok(())
    }
}
