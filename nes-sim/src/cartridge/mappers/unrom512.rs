use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// https://www.nesdev.org/wiki/UNROM_512
// 与UNROM同构：$8000-$BFFF切换16K bank，$C000-$FFFF固定最后16K；
// latch位6-5选择8K CHR RAM bank，位7在单屏/HV子模式里选择命名表。
const PRG_BANK_LEN: usize = 0x4000;
// 板载CHR RAM上限32K；iNES 1.0头未声明时也按32K处理
const CHR_RAM_MAX: usize = 0x8000;

enum NametableMode {
    HardHorizontal,
    HardVertical,
    // 头部flags6位1置位、位0清零：latch位7选择两个CIRAM页
    OneScreen,
    // 头部flags6位1、位0均置位：命名表占用CHR RAM末8K
    FourScreenVram,
    // submapper 3：latch位7在H/V镜像间切换（头部镜像位被忽略）
    SwitchableHv,
}

pub(super) struct Unrom512 {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    latch: u8,
    nametable_mode: NametableMode,
    bus_conflicts: bool,
    // false时$8000-$BFFF写入属于flash区（未模拟），不修改latch
    latch_writable_low: bool,
}

impl Unrom512 {
    pub(super) fn new(
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
        chr_ram_len: usize,
        flags6: u8,
        submapper: u8,
    ) -> Self {
        // cartridge.rs用255表示非NES 2.0头
        let submapper = if submapper == 255 { 0 } else { submapper };
        // 板子只有CHR RAM；头部若带CHR ROM则忽略
        let _ = chr_rom;
        // iNES 1.0头（submapper=255）默认32K CHR RAM；NES 2.0按声明容量（8K-32K）
        let chr_ram_len = if submapper == 255 {
            CHR_RAM_MAX
        } else {
            chr_ram_len.clamp(CHR_BANK_LEN, CHR_RAM_MAX)
        };

        let nametable_mode = if submapper == 3 {
            NametableMode::SwitchableHv
        } else {
            match flags6 & 0x03 {
                0b00 => NametableMode::HardHorizontal,
                0b01 => NametableMode::HardVertical,
                0b10 => NametableMode::OneScreen,
                _ => NametableMode::FourScreenVram,
            }
        };
        // submapper 0（无电池位）与submapper 2有总线冲突且latch全范围可写；
        // 带电池位的submapper 0及submapper 1/3/4无冲突，latch仅在$C000-$FFFF可写
        let (bus_conflicts, latch_writable_low) = match submapper {
            0 => (flags6 & 0x02 == 0, flags6 & 0x02 == 0),
            2 => (true, true),
            _ => (false, false),
        };

        Self {
            prg_rom,
            chr_ram: vec![0; chr_ram_len],
            latch: 0,
            nametable_mode,
            bus_conflicts,
            latch_writable_low,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let bank = if addr < 0xC000 {
            (self.latch & 0x1F) as usize % self.prg_bank_count()
        } else {
            self.prg_bank_count() - 1
        };
        bank * PRG_BANK_LEN + (addr as usize - 0x8000) % PRG_BANK_LEN
    }

    fn chr_index(&self, addr: u16) -> usize {
        let bank_count = (self.chr_ram.len() / CHR_BANK_LEN).max(1);
        let bank = ((self.latch >> 5) & 0x03) as usize % bank_count;
        bank * CHR_BANK_LEN + addr as usize
    }

    fn nt_vram_offset(&self) -> usize {
        self.chr_ram.len() - CHR_BANK_LEN
    }
}

impl Mapper for Unrom512 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                if addr < 0xC000 && !self.latch_writable_low {
                    return true;
                }
                self.latch = if self.bus_conflicts {
                    data & self.prg_rom[self.prg_rom_index(addr)]
                } else {
                    data
                };
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x0000..=0x1FFF => Some(self.chr_ram[self.chr_index(addr)]),
            _ => None,
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_index(addr);
                self.chr_ram[index] = data;
                true
            }
            _ => false,
        }
    }

    fn mirroring(&self) -> Mirroring {
        let upper = self.latch & 0x80 != 0;
        match self.nametable_mode {
            NametableMode::HardHorizontal => Mirroring::Horizontal,
            NametableMode::HardVertical => Mirroring::Vertical,
            NametableMode::OneScreen => {
                if upper {
                    Mirroring::SPAGE1
                } else {
                    Mirroring::SPAGE0
                }
            }
            NametableMode::SwitchableHv => {
                if upper {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                }
            }
            NametableMode::FourScreenVram => Mirroring::FourScreen,
        }
    }

    // 4屏模式下命名表存放在CHR RAM末8K，绕过主机CIRAM
    fn ppu_read_nametable(&mut self, addr: u16) -> Option<u8> {
        if matches!(self.nametable_mode, NametableMode::FourScreenVram) {
            Some(self.chr_ram[self.nt_vram_offset() + (addr as usize & 0x0FFF)])
        } else {
            None
        }
    }

    fn ppu_write_nametable(&mut self, addr: u16, data: u8) -> bool {
        if matches!(self.nametable_mode, NametableMode::FourScreenVram) {
            let index = self.nt_vram_offset() + (addr as usize & 0x0FFF);
            self.chr_ram[index] = data;
            true
        } else {
            false
        }
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
