use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;

pub(super) struct Mapper94 {
    prg_rom: Vec<u8>,
    prg_bank: usize,
    chr: Vec<u8>,
    mirroring: Mirroring,
}

impl Mapper94 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            prg_bank: 0,
            chr: if chr_rom.is_empty() {
                vec![0; 0x2000]
            } else {
                chr_rom
            },
            mirroring,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_16K
    }
}

impl Mapper for Mapper94 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.prg_bank % self.prg_bank_count_16k();
                let offset = bank * PRG_BANK_16K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xFFFF => {
                let last = self.prg_bank_count_16k().saturating_sub(1);
                let offset = last * PRG_BANK_16K + (addr as usize - 0xC000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.prg_bank = ((data >> 2) & 0x07) as usize;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        Some(self.chr[addr as usize % self.chr.len()])
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let len = self.chr.len();
        self.chr[addr as usize % len] = data;
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.prg_bank as u8);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank = reader.read_u8()? as usize;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
