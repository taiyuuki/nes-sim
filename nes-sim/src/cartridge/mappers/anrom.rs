use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_LEN: usize = 0x8000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Anrom {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    selected_prg_bank: usize,
    mirroring: Mirroring,
}

impl Anrom {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, _mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr,
            selected_prg_bank: 0,
            mirroring: Mirroring::SPAGE0,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let bank = self.selected_prg_bank % self.prg_bank_count();
        let offset = addr as usize - 0x8000;
        (bank * PRG_BANK_LEN + offset) % self.prg_rom.len()
    }
}

impl Mapper for Anrom {
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
                self.prg_ram[(addr - 0x6000) as usize] = data;
                true
            }
            0x8000..=0xFFFF => {
                self.selected_prg_bank = (data & 0x0F) as usize;
                self.mirroring = if data & 0x10 == 0 {
                    Mirroring::SPAGE0
                } else {
                    Mirroring::SPAGE1
                };
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match (&mut self.chr, addr) {
            (ChrMemory::Rom(chr_rom), 0x0000..=0x1FFF) => Some(chr_rom[addr as usize]),
            (ChrMemory::Ram(chr_ram), 0x0000..=0x1FFF) => Some(chr_ram[addr as usize]),
            _ => None,
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        match (&mut self.chr, addr) {
            (ChrMemory::Ram(chr_ram), 0x0000..=0x1FFF) => {
                chr_ram[addr as usize] = data;
                true
            }
            (ChrMemory::Rom(_), 0x0000..=0x1FFF) => true,
            _ => false,
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        writer.write_u64(self.selected_prg_bank as u64);
        writer.write_u8(match self.mirroring {
            Mirroring::SPAGE0 => 0,
            Mirroring::SPAGE1 => 1,
            _ => 0,
        });
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        self.selected_prg_bank = reader.read_u64()? as usize;
        self.mirroring = match reader.read_u8()? {
            0 => Mirroring::SPAGE0,
            1 => Mirroring::SPAGE1,
            _ => {
                return Err(SaveStateError::InvalidData(
                    "invalid mirroring mode for ANROM save state",
                ));
            }
        };
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for ANROM save state",
                ));
            }
        }
        Ok(())
    }
}
