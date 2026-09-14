use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_4K: usize = 0x1000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// MMC2 board (iNES mapper 009, Punch-Out!!).
///
/// The CHR latch switches each 4 KiB pattern table between two bank registers
/// whenever the PPU fetches tile $FD or $FE.
pub(super) struct Mmc2 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    prg_bank: u8,
    chr_banks: [u8; 4],
    latch_fd: [bool; 2],
    mirroring: Mirroring,
}

impl Mmc2 {
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
            prg_bank: 0,
            chr_banks: [0; 4],
            latch_fd: [true, false],
            mirroring,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn chr_bank_count_4k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_4K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_4K).max(1),
        }
    }
}

impl Mapper for Mmc2 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0x9FFF => {
                let bank = (self.prg_bank & 0x0F) as usize % self.prg_bank_count_8k();
                Some(self.prg_rom[bank * PRG_BANK_8K + (addr as usize - 0x8000)])
            }
            0xA000..=0xFFFF => {
                let fixed = self.prg_rom.len() - (0x10000 - 0xA000);
                Some(self.prg_rom[fixed + (addr as usize - 0xA000)])
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
            0xA000..=0xAFFF => {
                self.prg_bank = data;
                true
            }
            0xB000..=0xBFFF => {
                self.chr_banks[0] = data & 0x1F;
                true
            }
            0xC000..=0xCFFF => {
                self.chr_banks[1] = data & 0x1F;
                true
            }
            0xD000..=0xDFFF => {
                self.chr_banks[2] = data & 0x1F;
                true
            }
            0xE000..=0xEFFF => {
                self.chr_banks[3] = data & 0x1F;
                true
            }
            0xF000..=0xFFFF => {
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
        match addr {
            0x0FD8..=0x0FDF => self.latch_fd[0] = true,
            0x0FE8..=0x0FEF => self.latch_fd[0] = false,
            0x1FD8..=0x1FDF => self.latch_fd[1] = true,
            0x1FE8..=0x1FEF => self.latch_fd[1] = false,
            _ => {}
        }
        let table = addr as usize / CHR_BANK_4K;
        let reg_index = table * 2 + usize::from(!self.latch_fd[table]);
        let bank = self.chr_banks[reg_index] as usize % self.chr_bank_count_4k();
        let offset = bank * CHR_BANK_4K + (addr as usize & 0x0FFF);
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let table = addr as usize / CHR_BANK_4K;
        let reg_index = table * 2 + usize::from(!self.latch_fd[table]);
        let bank = self.chr_banks[reg_index] as usize % self.chr_bank_count_4k();
        let offset = bank * CHR_BANK_4K + (addr as usize & 0x0FFF);
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
        writer.write_bytes(&self.prg_ram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        writer.write_u8(self.prg_bank);
        writer.write_bytes(&self.chr_banks);
        for &latch in &self.latch_fd {
            writer.write_bool(latch);
        }
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
                    "CHR RAM mismatch for MMC2 save state",
                ));
            }
        }
        self.prg_bank = reader.read_u8()?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        for latch in &mut self.latch_fd {
            *latch = reader.read_bool()?;
        }
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
