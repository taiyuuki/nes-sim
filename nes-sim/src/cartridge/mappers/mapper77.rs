use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// https://www.nesdev.org/wiki/INES_0077
// IREM LROG017/LROG057H：$8000-$FFFF写入经总线冲突进latch；
// 低3位选32K PRG bank，高4位选$0000-$07FF的2K CHR ROM bank，
// $0800-$1FFF固定映射板载6K CHR RAM（三个2K窗口线性排布）。
const PRG_BANK_LEN: usize = 0x8000;
const CHR_RAM_LEN: usize = 0x1800;

pub(super) struct Mapper77 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    // CHR-ROM存在时6K（$0800-$1FFF）；纯CHR-RAM卡带8K覆盖全窗口
    chr_ram: Vec<u8>,
    latch: u8,
    mirroring: Mirroring,
}

impl Mapper77 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr_ram = if chr_rom.is_empty() {
            vec![0; CHR_BANK_LEN]
        } else {
            vec![0; CHR_RAM_LEN]
        };
        Self {
            prg_rom,
            chr_rom,
            chr_ram,
            latch: 0,
            mirroring,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let bank = (self.latch & 0x07) as usize % self.prg_bank_count();
        bank * PRG_BANK_LEN + (addr as usize - 0x8000)
    }
}

impl Mapper for Mapper77 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                // 总线冲突：写入值与当前ROM输出按位与（Nestopia GetBusData）
                self.latch = data & self.prg_rom[self.prg_rom_index(addr)];
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x0000..=0x07FF if !self.chr_rom.is_empty() => {
                let bank_count = (self.chr_rom.len() / 0x0800).max(1);
                let bank = (self.latch >> 4) as usize % bank_count;
                Some(self.chr_rom[bank * 0x0800 + addr as usize])
            }
            0x0000..=0x07FF => Some(self.chr_ram[addr as usize]),
            0x0800..=0x1FFF => Some(self.chr_ram[addr as usize - 0x0800]),
            _ => None,
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x0000..=0x07FF if self.chr_rom.is_empty() => {
                self.chr_ram[addr as usize] = data;
                true
            }
            0x0800..=0x1FFF => {
                self.chr_ram[addr as usize - 0x0800] = data;
                true
            }
            _ => false,
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.latch);
        writer.write_bytes(&self.chr_ram);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.latch = reader.read_u8()?;
        reader.read_bytes_into(&mut self.chr_ram)?;
        Ok(())
    }
}
