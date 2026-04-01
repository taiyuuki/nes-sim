use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_LEN: usize = 0x4000;
const CHR_HALF_BANK_LEN: usize = 0x1000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mmc1 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    shift_register: u8,
    control: u8,
    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,
    four_screen: bool,
}

impl Mmc1 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        let mut control = 0x0C;
        if matches!(mirroring, Mirroring::FourScreen) {
            control &= !0x03;
        }

        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr,
            shift_register: 0x10,
            control,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            four_screen: matches!(mirroring, Mirroring::FourScreen),
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_LEN
    }

    fn chr_half_bank_count(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len() / CHR_HALF_BANK_LEN,
            ChrMemory::Ram(chr_ram) => chr_ram.len() / CHR_HALF_BANK_LEN,
        }
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let bank_mode = (self.control >> 2) & 0x03;
        let offset = addr as usize & 0x3FFF;

        match bank_mode {
            0 | 1 => {
                let bank = (self.prg_bank as usize & !0x01) % self.prg_bank_count();
                (bank * PRG_BANK_LEN) + (addr as usize - 0x8000)
            }
            2 => {
                if addr < 0xC000 {
                    offset
                } else {
                    let bank = self.prg_bank as usize % self.prg_bank_count();
                    bank * PRG_BANK_LEN + offset
                }
            }
            3 => {
                if addr < 0xC000 {
                    let bank = self.prg_bank as usize % self.prg_bank_count();
                    bank * PRG_BANK_LEN + offset
                } else {
                    let last_bank = self.prg_bank_count() - 1;
                    last_bank * PRG_BANK_LEN + offset
                }
            }
            _ => unreachable!(),
        }
    }

    fn chr_index(&self, addr: u16) -> usize {
        let chr_mode = (self.control >> 4) & 0x01;
        let offset = addr as usize & 0x0FFF;

        if chr_mode == 0 {
            let bank = (self.chr_bank_0 as usize & !0x01) % self.chr_half_bank_count();
            bank * CHR_HALF_BANK_LEN + addr as usize
        } else if addr < 0x1000 {
            let bank = self.chr_bank_0 as usize % self.chr_half_bank_count();
            bank * CHR_HALF_BANK_LEN + offset
        } else {
            let bank = self.chr_bank_1 as usize % self.chr_half_bank_count();
            bank * CHR_HALF_BANK_LEN + offset
        }
    }

    fn commit_shift_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => self.control = value & 0x1F,
            0xA000..=0xBFFF => self.chr_bank_0 = value & 0x1F,
            0xC000..=0xDFFF => self.chr_bank_1 = value & 0x1F,
            0xE000..=0xFFFF => self.prg_bank = value & 0x1F,
            _ => {}
        }
    }
}

impl Mapper for Mmc1 {
    fn mapper_id(&self) -> u16 {
        1
    }

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
                if data & 0x80 != 0 {
                    self.shift_register = 0x10;
                    self.control |= 0x0C;
                    return true;
                }

                let complete = (self.shift_register & 0x01) != 0;
                self.shift_register >>= 1;
                self.shift_register |= (data & 0x01) << 4;

                if complete {
                    self.commit_shift_register(addr, self.shift_register);
                    self.shift_register = 0x10;
                }
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }

        let index = self.chr_index(addr);
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[index]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[index]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }

        let index = self.chr_index(addr);
        match &mut self.chr {
            ChrMemory::Ram(chr_ram) => {
                chr_ram[index] = data;
                true
            }
            ChrMemory::Rom(_) => true,
        }
    }

    fn mirroring(&self) -> Mirroring {
        if self.four_screen {
            return Mirroring::FourScreen;
        }

        match self.control & 0x03 {
            0 => Mirroring::SPAGE0,
            1 => Mirroring::SPAGE1,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        writer.write_u8(self.shift_register);
        writer.write_u8(self.control);
        writer.write_u8(self.chr_bank_0);
        writer.write_u8(self.chr_bank_1);
        writer.write_u8(self.prg_bank);
        writer.write_bool(self.four_screen);
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
        self.shift_register = reader.read_u8()?;
        self.control = reader.read_u8()?;
        self.chr_bank_0 = reader.read_u8()?;
        self.chr_bank_1 = reader.read_u8()?;
        self.prg_bank = reader.read_u8()?;
        self.four_screen = reader.read_bool()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for MMC1 save state",
                ));
            }
        }
        Ok(())
    }
}
