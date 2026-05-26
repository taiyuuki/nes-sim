use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_2K: usize = 0x0800;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Namco3433 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    command: u8,
    regs: [u8; 8],
    mirroring: Mirroring,
    single_screen: bool,
}

impl Namco3433 {
    pub(super) fn new(
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
        mirroring: Mirroring,
        single_screen: bool,
    ) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            command: 0,
            regs: [0; 8],
            mirroring,
            single_screen,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K).max(1)
    }

    fn chr_bank_count_2k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_2K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_2K).max(1),
        }
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_1K).max(1),
        }
    }
}

impl Mapper for Namco3433 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0x9FFF => {
                let bank = self.regs[6] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xA000..=0xBFFF => {
                let bank = self.regs[7] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0xA000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xDFFF => {
                let second_last = self.prg_bank_count_8k().saturating_sub(2);
                let offset = second_last * PRG_BANK_8K + (addr as usize - 0xC000);
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
        match addr & 0x8001 {
            0x8000 => {
                self.command = data & 0x07;
                let mirror_bits = (data >> 6) & 0x03;
                self.mirroring = if self.single_screen {
                    if mirror_bits & 1 != 0 {
                        Mirroring::SPAGE1
                    } else {
                        Mirroring::SPAGE0
                    }
                } else {
                    match mirror_bits {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SPAGE0,
                        3 => Mirroring::SPAGE1,
                        _ => unreachable!(),
                    }
                };
            }
            0x8001 => {
                self.regs[self.command as usize] = data;
            }
            _ => return false,
        }
        true
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let offset = addr as usize;
        let max_2k = self.chr_bank_count_2k();
        let max_1k = self.chr_bank_count_1k();
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => {
                let (bank, bank_size, sub_offset) = match offset {
                    0x0000..=0x07FF => (self.regs[0] as usize >> 1, CHR_BANK_2K, offset & 0x07FF),
                    0x0800..=0x0FFF => (self.regs[1] as usize >> 1, CHR_BANK_2K, offset & 0x07FF),
                    0x1000..=0x13FF => {
                        ((self.regs[2] as usize) | 0x40, CHR_BANK_1K, offset & 0x03FF)
                    }
                    0x1400..=0x17FF => {
                        ((self.regs[3] as usize) | 0x40, CHR_BANK_1K, offset & 0x03FF)
                    }
                    0x1800..=0x1BFF => {
                        ((self.regs[4] as usize) | 0x40, CHR_BANK_1K, offset & 0x03FF)
                    }
                    _ => ((self.regs[5] as usize) | 0x40, CHR_BANK_1K, offset & 0x03FF),
                };
                let max_bank = if bank_size == CHR_BANK_2K {
                    max_2k
                } else {
                    max_1k
                };
                let bank = bank % max_bank;
                Some(chr_rom[(bank * bank_size + sub_offset) % chr_rom.len()])
            }
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[addr as usize] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.command);
        writer.write_bytes(&self.regs);
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_bool(self.single_screen);
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
        reader.read_bytes_into(&mut self.regs)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.single_screen = reader.read_bool()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Namco 3433 save state",
                ));
            }
        }
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
            "invalid Namco 3433 mirroring value",
        )),
    }
}
