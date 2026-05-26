use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;

pub(super) struct Camerica {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    prg_bank: usize,
    mirroring: Mirroring,
    hardwired_mirroring: Mirroring,
    use_mapper_mirroring: bool,
}

impl Camerica {
    pub(super) fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_ram: vec![0; CHR_BANK_LEN],
            prg_bank: 0,
            mirroring,
            hardwired_mirroring: mirroring,
            use_mapper_mirroring: false,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_16K
    }
}

impl Mapper for Camerica {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.prg_bank % self.prg_bank_count().max(1);
                let offset = bank * PRG_BANK_16K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xFFFF => {
                let last = self.prg_bank_count().saturating_sub(1);
                let offset = last * PRG_BANK_16K + (addr as usize - 0xC000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                if (addr & 0xF000) == 0x9000 {
                    self.mirroring = if (data >> 4) & 1 != 0 {
                        Mirroring::SPAGE1
                    } else {
                        Mirroring::SPAGE0
                    };
                    self.use_mapper_mirroring = true;
                } else {
                    self.prg_bank = data as usize;
                }
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x0000..=0x1FFF => Some(self.chr_ram[addr as usize]),
            _ => None,
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => {
                self.chr_ram[addr as usize] = data;
                true
            }
            _ => false,
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u64(self.prg_bank as u64);
        writer.write_bool(self.use_mapper_mirroring);
        if self.use_mapper_mirroring {
            writer.write_u8(encode_mirroring(self.mirroring));
        }
        writer.write_bytes(&self.chr_ram);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank = reader.read_u64()? as usize;
        self.use_mapper_mirroring = reader.read_bool()?;
        if self.use_mapper_mirroring {
            self.mirroring = decode_mirroring(reader.read_u8()?)?;
        } else {
            self.mirroring = self.hardwired_mirroring;
        }
        reader.read_bytes_into(&mut self.chr_ram)?;
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
        _ => Err(SaveStateError::InvalidData(
            "invalid Camerica mirroring value",
        )),
    }
}
