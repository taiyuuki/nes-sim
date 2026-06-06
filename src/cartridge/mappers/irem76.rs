use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_2K: usize = 0x0800;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Irem76 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    command: u8,
    chr_banks: [u8; 4],
    prg_banks: [u8; 2],
    mirroring: Mirroring,
}

impl Irem76 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            command: 0,
            chr_banks: [0; 4],
            prg_banks: [0; 2],
            mirroring,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn chr_bank_count_2k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_2K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_2K).max(1),
        }
    }
}

impl Mapper for Irem76 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0x9FFF => {
                let bank = self.prg_banks[0] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xA000..=0xBFFF => {
                let bank = self.prg_banks[1] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0xA000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xFFFF => {
                let base = self.prg_bank_count_8k().saturating_sub(2);
                let offset = base * PRG_BANK_8K + (addr as usize - 0xC000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000 => {
                self.command = data & 0x07;
                true
            }
            0x8001 => match self.command {
                0..=3 => {
                    self.chr_banks[self.command as usize] = data;
                    true
                }
                4 => {
                    // chr_banks[0] and [1] are set but not used (Namco 109 legacy)
                    self.chr_banks[0] = data;
                    true
                }
                5 => {
                    self.chr_banks[1] = data;
                    true
                }
                6 => {
                    self.prg_banks[0] = data;
                    true
                }
                7 => {
                    self.prg_banks[1] = data;
                    true
                }
                _ => unreachable!(),
            },
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let slot = (addr as usize / CHR_BANK_2K) % 4;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_2k();
        let offset = bank * CHR_BANK_2K + (addr as usize & 0x07FF);
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = (addr as usize / CHR_BANK_2K) % 4;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_2k();
        let offset = bank * CHR_BANK_2K + (addr as usize & 0x07FF);
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
        writer.write_u8(self.command);
        writer.write_bytes(&self.chr_banks);
        writer.write_bytes(&self.prg_banks);
        writer.write_u8(encode_mirroring(self.mirroring));
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.command = reader.read_u8()?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        reader.read_bytes_into(&mut self.prg_banks)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Irem 76 save state",
                ));
            }
        }
        Ok(())
    }
}
