use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Vrc2 {
    prg_rom: Vec<u8>,
    chr_memory: ChrMemory,
    prg_bank_0: u8,
    prg_bank_1: u8,
    chr_banks: [u8; 8],
    mirroring: Mirroring,
}

impl Vrc2 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_data: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr_memory = if chr_data.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_data)
        };

        Self {
            prg_rom,
            chr_memory,
            prg_bank_0: 0,
            prg_bank_1: 1,
            chr_banks: [0, 1, 2, 3, 4, 5, 6, 7],
            mirroring,
        }
    }
}

impl Mapper for Vrc2 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let second_last_8k = self.prg_rom.len().saturating_sub(PRG_BANK_8K * 2);

                let offset = match addr {
                    0x8000..=0x9FFF => {
                        let bank = self.prg_bank_0 as usize;
                        (addr - 0x8000) as usize + bank * PRG_BANK_8K
                    }
                    0xA000..=0xBFFF => {
                        let bank = self.prg_bank_1 as usize;
                        (addr - 0xA000) as usize + bank * PRG_BANK_8K
                    }
                    _ => (addr - 0xC000) as usize + second_last_8k,
                };
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                let bit0 = (addr & 0x02) != 0;
                let bit1 = (addr & 0x01) != 0;
                match addr & 0xF000 {
                    0x8000 => {
                        self.prg_bank_0 = data & 0x0F;
                    }
                    0x9000 => {
                        self.mirroring = if (data & 0x01) != 0 {
                            Mirroring::Horizontal
                        } else {
                            Mirroring::Vertical
                        };
                    }
                    0xA000 => {
                        self.prg_bank_1 = data & 0x0F;
                    }
                    0xB000 | 0xC000 | 0xD000 | 0xE000 => {
                        let data = data & 0x0F;
                        let which_reg =
                            ((addr as usize - 0xB000) >> 11) as u8 + if bit1 { 1 } else { 0 };
                        let old_val = self.chr_banks[which_reg as usize];
                        if bit0 {
                            self.chr_banks[which_reg as usize] = (old_val & 0x0F) | (data << 4);
                        } else {
                            self.chr_banks[which_reg as usize] = (old_val & 0xF0) | data;
                        }
                    }
                    _ => {}
                }
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        let addr = addr as usize & 0x1FFF;
        match &self.chr_memory {
            ChrMemory::Rom(rom) => {
                let bank = (self.chr_banks[addr / CHR_BANK_1K] >> 1) as usize;
                let offset = (addr % CHR_BANK_1K) + bank * CHR_BANK_1K;
                Some(rom[offset % rom.len()])
            }
            ChrMemory::Ram(ram) => Some(ram[addr]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if let ChrMemory::Ram(ram) = &mut self.chr_memory {
            ram[addr as usize & 0x1FFF] = data;
            return true;
        }
        false
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.prg_bank_0);
        writer.write_u8(self.prg_bank_1);
        for &bank in &self.chr_banks {
            writer.write_u8(bank);
        }
        writer.write_u8(self.mirroring as u8);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank_0 = reader.read_u8()?;
        self.prg_bank_1 = reader.read_u8()?;
        for bank in &mut self.chr_banks {
            *bank = reader.read_u8()?;
        }
        self.mirroring = match reader.read_u8()? {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::FourScreen,
            3 => Mirroring::SPAGE0,
            4 => Mirroring::SPAGE1,
            _ => {
                return Err(SaveStateError::InvalidData(
                    "invalid mirroring value in VRC2 state",
                ));
            }
        };
        Ok(())
    }
}
