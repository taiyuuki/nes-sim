use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// https://www.nesdev.org/wiki/Action_53_mapper
// 写$5000-$5FFF选择寄存器（D7=超级用户，D0=寄存器号，实际只有
// $00/$01/$80/$81四个），之后写$8000-$FFFF写入所选寄存器。
const PRG_BANK_LEN: usize = 0x4000;
// 卡带自带32K CHR RAM（4个8K bank由$00寄存器低位选择）
const CHR_RAM_LEN: usize = 0x8000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Action53 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    // 当前选中的寄存器（写入$5000-$5FFF的值 & 0x81）
    select: u8,
    // 寄存器$00：CHR bank（低2位）
    chr_bank: u8,
    // 寄存器$01：内部PRG bank（低4位）
    inner_bank: u8,
    // 寄存器$80：位1-0镜像模式，位3-2 PRG模式，位5-4外部bank容量
    mode: u8,
    // 寄存器$81：外部PRG bank
    outer_bank: u8,
}

impl Action53 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, _mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_RAM_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        // 上电状态与FCEUX/Nestopia一致：保证$C000映射ROM最后16K
        Self {
            prg_rom,
            chr,
            select: 0x00,
            chr_bank: 0x00,
            inner_bank: 0x0F,
            mode: 0x00,
            outer_bank: 0x3F,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn chr_bank_count(&self) -> usize {
        let len = match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len(),
            ChrMemory::Ram(chr_ram) => chr_ram.len(),
        };
        (len / CHR_BANK_LEN).max(1)
    }

    // 返回$8000/$C000窗口的16K bank号（未按实际ROM容量取模）
    fn prg_16k_banks(&self) -> (usize, usize) {
        let outer = (self.outer_bank as usize) << 1;
        let inner = self.inner_bank as usize;
        match self.mode & 0x3C {
            // 32K模式：inner低位选择32K bank，inner自身被忽略
            0x00 | 0x04 => (outer, outer | 1),
            0x10 | 0x14 => {
                let bank = (outer & !0x2) | ((inner << 1) & 0x2);
                (bank, bank | 1)
            }
            0x20 | 0x24 => {
                let bank = (outer & !0x6) | ((inner << 1) & 0x6);
                (bank, bank | 1)
            }
            0x30 | 0x34 => {
                let bank = (outer & !0xE) | ((inner << 1) & 0xE);
                (bank, bank | 1)
            }
            // 底部固定（UNROM #180风格）
            0x08 => (outer, outer | (inner & 0x1)),
            0x18 => (outer, (outer & !0x2) | (inner & 0x3)),
            0x28 => (outer, (outer & !0x6) | (inner & 0x7)),
            0x38 => (outer, (outer & !0xE) | (inner & 0xF)),
            // 顶部固定（UNROM #2风格）
            0x0C => (outer | (inner & 0x1), outer | 1),
            0x1C => ((outer & !0x2) | (inner & 0x3), outer | 1),
            0x2C => ((outer & !0x6) | (inner & 0x7), outer | 1),
            0x3C => ((outer & !0xE) | (inner & 0xF), outer | 1),
            _ => (outer, outer | 1),
        }
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let (lo, hi) = self.prg_16k_banks();
        let bank = if addr < 0xC000 { lo } else { hi } % self.prg_bank_count();
        bank * PRG_BANK_LEN + (addr as usize - 0x8000) % PRG_BANK_LEN
    }

    fn chr_index(&self, addr: u16) -> usize {
        let bank = self.chr_bank as usize % self.chr_bank_count();
        bank * CHR_BANK_LEN + addr as usize
    }

    // 单屏镜像模式下$00/$01写入的D4切换镜像页
    fn update_mirroring_bit(&mut self, data: u8) {
        if self.mode & 0x02 == 0 {
            self.mode = (self.mode & 0xFE) | ((data >> 4) & 1);
        }
    }
}

impl Mapper for Action53 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x5000..=0x5FFF => {
                self.select = data & 0x81;
                true
            }
            0x8000..=0xFFFF => {
                match self.select {
                    0x00 => {
                        self.chr_bank = data & 0x03;
                        self.update_mirroring_bit(data);
                    }
                    0x01 => {
                        self.inner_bank = data & 0x0F;
                        self.update_mirroring_bit(data);
                    }
                    0x80 => self.mode = data & 0x3F,
                    0x81 => self.outer_bank = data,
                    _ => {}
                }
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match (&self.chr, addr) {
            (ChrMemory::Rom(chr_rom), 0x0000..=0x1FFF) => Some(chr_rom[self.chr_index(addr)]),
            (ChrMemory::Ram(chr_ram), 0x0000..=0x1FFF) => Some(chr_ram[self.chr_index(addr)]),
            _ => None,
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => {
                let index = self.chr_index(addr);
                match &mut self.chr {
                    ChrMemory::Ram(chr_ram) => {
                        chr_ram[index] = data;
                        true
                    }
                    ChrMemory::Rom(_) => true,
                }
            }
            _ => false,
        }
    }

    fn mirroring(&self) -> Mirroring {
        match self.mode & 0x03 {
            0 => Mirroring::SPAGE0,
            1 => Mirroring::SPAGE1,
            2 => Mirroring::Vertical,
            _ => Mirroring::Horizontal,
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.select);
        writer.write_u8(self.chr_bank);
        writer.write_u8(self.inner_bank);
        writer.write_u8(self.mode);
        writer.write_u8(self.outer_bank);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.select = reader.read_u8()?;
        self.chr_bank = reader.read_u8()?;
        self.inner_bank = reader.read_u8()?;
        self.mode = reader.read_u8()?;
        self.outer_bank = reader.read_u8()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for Action 53 save state",
                ));
            }
        }
        Ok(())
    }
}
