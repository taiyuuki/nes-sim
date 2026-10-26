use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// https://www.nesdev.org/wiki/INES_0234
// AVE D1012（Maxi 15 multicart）：读触发寄存器。
// 首次读$FF80-$FF9F把该处ROM字节锁存为bank寄存器；每次读$FFE8-$FFF7
// 锁存为preg。bank位7选镜像，位6决定PRG/CHR组合方式。
const PRG_BANK_LEN: usize = 0x8000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper234 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    bank: u8,
    preg: u8,
}

impl Mapper234 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, _mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            bank: 0,
            preg: 0,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn prg_bank_number(&self) -> usize {
        let value = if self.bank & 0x40 != 0 {
            (self.bank & 0x0E) | (self.preg & 0x01)
        } else {
            self.bank & 0x0F
        };
        value as usize % self.prg_bank_count()
    }

    fn chr_bank_number(&self) -> usize {
        let value = if self.bank & 0x40 != 0 {
            ((self.bank & 0x0E) << 2) | ((self.preg >> 4) & 0x07)
        } else {
            ((self.bank & 0x0F) << 2) | ((self.preg >> 4) & 0x03)
        };
        let count = match &self.chr {
            ChrMemory::Rom(chr_rom) => (chr_rom.len() / CHR_BANK_LEN).max(1),
            ChrMemory::Ram(_) => 1,
        };
        value as usize % count
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        self.prg_bank_number() * PRG_BANK_LEN + (addr as usize - 0x8000)
    }
}

impl Mapper for Mapper234 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0xFF80..=0xFF9F => {
                let value = self.prg_rom[self.prg_rom_index(addr)];
                if self.bank == 0 {
                    self.bank = value;
                }
                Some(value)
            }
            0xFFE8..=0xFFF7 => {
                let value = self.prg_rom[self.prg_rom_index(addr)];
                self.preg = value;
                Some(value)
            }
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match (&self.chr, addr) {
            (ChrMemory::Rom(chr_rom), 0x0000..=0x1FFF) => {
                Some(chr_rom[self.chr_bank_number() * CHR_BANK_LEN + addr as usize])
            }
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
        if self.bank & 0x80 == 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.bank);
        writer.write_u8(self.preg);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.bank = reader.read_u8()?;
        self.preg = reader.read_u8()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for AVE D1012 save state",
                ));
            }
        }
        Ok(())
    }
}
