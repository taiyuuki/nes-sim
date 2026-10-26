use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// https://www.nesdev.org/wiki/INES_0232
// Camerica BF9096 四合一卡带：BF9093（mapper 71）加外层bank。
// 外层寄存器选择64K游戏区（按物理顺序），内层寄存器选择区内16K bank，
// $C000-$FFFF固定在外层区的最后一个bank。
const PRG_BANK_LEN: usize = 0x4000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Quattro {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    // 外层区基址（16K bank号，0/4/8/12）
    outer_base: usize,
    // 内层16K bank（0-3）
    inner_bank: usize,
    mirroring: Mirroring,
}

impl Quattro {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            outer_base: 0,
            inner_bank: 0,
            mirroring,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let bank = if addr < 0xC000 {
            self.outer_base + self.inner_bank
        } else {
            self.outer_base + 3
        } % self.prg_bank_count();
        bank * PRG_BANK_LEN + (addr as usize - 0x8000) % PRG_BANK_LEN
    }
}

impl Mapper for Quattro {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xBFFF => {
                self.outer_base = ((data & 0x18) >> 1) as usize;
                true
            }
            0xC000..=0xFFFF => {
                self.inner_bank = (data & 0x03) as usize;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match (&self.chr, addr) {
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
        writer.write_u64(self.outer_base as u64);
        writer.write_u64(self.inner_bank as u64);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.outer_base = reader.read_u64()? as usize;
        self.inner_bank = reader.read_u64()? as usize;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for Quattro save state",
                ));
            }
        }
        Ok(())
    }
}
