use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_2K: usize = 0x0800;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Sunsoft-4 board (iNES mapper 068, Nantettatte!! Baseball / Maharaja).
///
/// Distinctive feature: two 1 KiB CHR banks can replace the nametables,
/// letting CHR-ROM serve as extra nametable data.
pub(super) struct Sunsoft4 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    chr_banks: [u8; 4],
    nt_banks: [u8; 2],
    use_crom_nt: bool,
    prg_bank: u8,
    mirroring: Mirroring,
}

impl Sunsoft4 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            chr_banks: [0; 4],
            nt_banks: [0; 2],
            use_crom_nt: false,
            prg_bank: 0,
            mirroring,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn chr_bank_count_2k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_2K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_2K).max(1),
        }
    }

    fn nt_page(&self, addr: u16) -> usize {
        match self.mirroring {
            Mirroring::Horizontal => usize::from((addr & 0x0800) != 0),
            Mirroring::Vertical => usize::from((addr & 0x0400) != 0),
            Mirroring::SPAGE0 => 0,
            Mirroring::SPAGE1 | Mirroring::FourScreen => 1,
        }
    }
}

impl Mapper for Sunsoft4 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank & 0x0F) as usize % self.prg_bank_count_16k();
                Some(self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0x8000)])
            }
            0xC000..=0xFFFF => {
                let last = self.prg_bank_count_16k() - 1;
                Some(self.prg_rom[last * PRG_BANK_16K + (addr as usize - 0xC000)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0x8FFF => {
                self.chr_banks[0] = data;
                true
            }
            0x9000..=0x9FFF => {
                self.chr_banks[1] = data;
                true
            }
            0xA000..=0xAFFF => {
                self.chr_banks[2] = data;
                true
            }
            0xB000..=0xBFFF => {
                self.chr_banks[3] = data;
                true
            }
            0xC000..=0xCFFF => {
                self.nt_banks[0] = data;
                true
            }
            0xD000..=0xDFFF => {
                self.nt_banks[1] = data;
                true
            }
            0xE000..=0xEFFF => {
                self.use_crom_nt = (data & 0x10) != 0;
                self.mirroring = match data & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SPAGE0,
                    _ => Mirroring::SPAGE1,
                };
                true
            }
            0xF000..=0xFFFF => {
                self.prg_bank = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let slot = addr as usize / CHR_BANK_2K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_2k();
        let offset = bank * CHR_BANK_2K + (addr as usize & 0x07FF);
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = addr as usize / CHR_BANK_2K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_2k();
        let offset = bank * CHR_BANK_2K + (addr as usize & 0x07FF);
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            let len = chr_ram.len();
            chr_ram[offset % len] = data;
        }
        true
    }

    fn ppu_read_nametable(&mut self, addr: u16) -> Option<u8> {
        if !self.use_crom_nt || !matches!(addr, 0x2000..=0x2FFF) {
            return None;
        }
        let page = self.nt_page(addr);
        let bank = self.nt_banks[page] as usize;
        let offset = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write_nametable(&mut self, addr: u16, _data: u8) -> bool {
        self.use_crom_nt && matches!(addr, 0x2000..=0x2FFF)
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        writer.write_bytes(&self.chr_banks);
        writer.write_bytes(&self.nt_banks);
        writer.write_bool(self.use_crom_nt);
        writer.write_u8(self.prg_bank);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Sunsoft-4 save state",
                ));
            }
        }
        reader.read_bytes_into(&mut self.chr_banks)?;
        reader.read_bytes_into(&mut self.nt_banks)?;
        self.use_crom_nt = reader.read_bool()?;
        self.prg_bank = reader.read_u8()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
