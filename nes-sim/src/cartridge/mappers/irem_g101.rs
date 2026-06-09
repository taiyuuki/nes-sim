use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const WRAM_LEN: usize = 8192;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct IremG101 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    wram: Vec<u8>,
    prg_banks: [u8; 2],
    chr_banks: [u8; 8],
    mirror_ctrl: u8,
}

impl IremG101 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            wram: vec![0; WRAM_LEN],
            prg_banks: [0, 1],
            chr_banks: [0; 8],
            mirror_ctrl: ((mirroring as u8) & 1) ^ 1,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_1K).max(1),
        }
    }

    fn get_mirroring(&self) -> Mirroring {
        if self.mirror_ctrl & 1 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn is_swapped(&self) -> bool {
        self.mirror_ctrl & 2 != 0
    }
}

impl Mapper for IremG101 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.wram[(addr & 0x1FFF) as usize]),
            0x8000..=0x9FFF | 0xC000..=0xDFFF => {
                let is_8000 = self.is_swapped() ^ ((addr & 0x4000) == 0);
                let (bank, base_addr) = if is_8000 {
                    (
                        self.prg_banks[0] as usize % self.prg_bank_count_8k(),
                        0x8000,
                    )
                } else {
                    (self.prg_bank_count_8k().saturating_sub(2), 0xC000)
                };
                let offset = bank * PRG_BANK_8K + (addr as usize - base_addr);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xA000..=0xBFFF => {
                let bank = self.prg_banks[1] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0xA000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xE000..=0xFFFF => {
                let last = self.prg_bank_count_8k().saturating_sub(1);
                let offset = last * PRG_BANK_8K + (addr as usize - 0xE000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                self.wram[(addr & 0x1FFF) as usize] = data;
                true
            }
            0x8000..=0x8FFF => {
                self.prg_banks[0] = data;
                true
            }
            0x9000..=0x9FFF => {
                self.mirror_ctrl = data;
                true
            }
            0xA000..=0xAFFF => {
                self.prg_banks[1] = data;
                true
            }
            0xB000..=0xBFFF => {
                self.chr_banks[(addr & 0x07) as usize] = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let slot = addr as usize / CHR_BANK_1K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_1k();
        let offset = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = addr as usize / CHR_BANK_1K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_1k();
        let offset = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            let len = chr_ram.len();
            chr_ram[offset % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.get_mirroring()
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_banks);
        writer.write_bytes(&self.chr_banks);
        writer.write_u8(self.mirror_ctrl);
        writer.write_bytes(&self.wram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_banks)?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        self.mirror_ctrl = reader.read_u8()?;
        reader.read_bytes_into(&mut self.wram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Irem G-101 save state",
                ));
            }
        }
        Ok(())
    }
}
