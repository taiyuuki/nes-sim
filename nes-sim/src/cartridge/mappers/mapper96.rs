use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// https://www.nesdev.org/wiki/INES_0096
// Bandai Oeka Kids：$8000-$FFFF写入的位1-0选32K PRG bank、位2为CHR高位；
// $0000的4K CHR bank低位来自PPU命名表地址总线的位9-8（读$2000-$2FFF时锁存），
// $1000固定为CHR高位|3。镜像固定单屏下页。
const PRG_BANK_LEN: usize = 0x8000;
const CHR_BANK_LEN_4K: usize = 0x1000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper96 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    reg: u8,
    // 最近一次命名表访问地址的位9-8
    ppulatch: u8,
}

impl Mapper96 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, _mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            reg: 0,
            ppulatch: 0,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let bank = (self.reg & 0x03) as usize % self.prg_bank_count();
        bank * PRG_BANK_LEN + (addr as usize - 0x8000)
    }

    fn chr_bank_4k(&self, window: usize) -> usize {
        let value = (self.reg & 0x04) | if window == 0 { self.ppulatch & 0x03 } else { 3 };
        let count = match &self.chr {
            ChrMemory::Rom(chr_rom) => (chr_rom.len() / CHR_BANK_LEN_4K).max(1),
            ChrMemory::Ram(_) => 2,
        };
        value as usize % count
    }
}

impl Mapper for Mapper96 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.reg = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match (&self.chr, addr) {
            (ChrMemory::Rom(chr_rom), 0x0000..=0x1FFF) => {
                let index = self.chr_bank_4k(addr as usize / CHR_BANK_LEN_4K) * CHR_BANK_LEN_4K
                    + (addr as usize & 0x0FFF);
                Some(chr_rom[index])
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
        Mirroring::SPAGE0
    }

    // 读命名表时把地址位9-8锁存进CHR bank低位，数据仍由主机CIRAM提供
    fn ppu_read_nametable(&mut self, addr: u16) -> Option<u8> {
        if matches!(addr, 0x2000..=0x2FFF) {
            self.ppulatch = ((addr >> 8) & 0x03) as u8;
        }
        None
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.reg);
        writer.write_u8(self.ppulatch);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.reg = reader.read_u8()?;
        self.ppulatch = reader.read_u8()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for Oeka Kids save state",
                ));
            }
        }
        Ok(())
    }
}
