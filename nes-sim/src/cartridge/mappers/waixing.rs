use super::mmc3::Mmc3Core;
use super::{Mapper, prg_ram_battery};
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// Waixing MMC3衍生板（iNES mapper 191/192/194/195）。与MMC3的差异仅在
// CHR：bank寄存器值命中特定范围时该1K bank来自板载CHR RAM而非CHR ROM。
// https://www.nesdev.org/wiki/INES_0191
const PRG_BANK_LEN: usize = 0x2000;
const CHR_BANK_LEN_1K: usize = 0x0400;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum WaixingVariant {
    // 191：bank值bit7=1 → 2K CHR RAM
    TypeB,
    // 192：bank值$08-$0B → 4K CHR RAM
    TypeC,
    // 194：bank值$00-$01 → 2K CHR RAM
    TypeD,
    // 195：bank值$00-$03 → 4K CHR RAM，且$5000-$5FFF另有4K PRG RAM窗口
    TypeE,
}

impl WaixingVariant {
    fn chr_ram_len(self) -> usize {
        match self {
            Self::TypeB | Self::TypeD => 0x0800,
            Self::TypeC | Self::TypeE => 0x1000,
        }
    }

    // 返回bank值命中的CHR RAM内偏移（1K粒度），未命中返回None
    fn chr_ram_bank(self, bank_value: u8) -> Option<usize> {
        match self {
            Self::TypeB if bank_value & 0x80 != 0 => Some((bank_value & 0x01) as usize),
            Self::TypeC if (8..=0x0B).contains(&bank_value) => Some((bank_value - 8) as usize),
            Self::TypeD if bank_value <= 1 => Some(bank_value as usize),
            Self::TypeE if bank_value <= 3 => Some(bank_value as usize),
            _ => None,
        }
    }
}

pub(super) struct Waixing {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_rom: Vec<u8>,
    // CHR-ROM存在时为板载CHR RAM（2K/4K）；纯CHR-RAM卡带则覆盖整个8K窗口
    chr_ram: Vec<u8>,
    chr_ram_full_window: bool,
    variant: WaixingVariant,
    mirroring: Mirroring,
    core: Mmc3Core,
}

impl Waixing {
    pub(super) fn new(
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
        mirroring: Mirroring,
        variant: WaixingVariant,
    ) -> Self {
        let chr_ram_full_window = chr_rom.is_empty();
        Self {
            prg_rom,
            // 195的$5000-$5FFF窗口映射到第三段4K（FCEUX setprg4r(0x10,0x5000,2)）
            prg_ram: vec![0; 0x4000],
            chr_rom,
            chr_ram: if chr_ram_full_window {
                vec![0; 0x2000]
            } else {
                vec![0; variant.chr_ram_len()]
            },
            chr_ram_full_window,
            variant,
            mirroring,
            core: Mmc3Core::new(),
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr_rom.len() / CHR_BANK_LEN_1K).max(1)
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let slot = ((addr - 0x8000) as usize) / PRG_BANK_LEN;
        let bank = self.core.prg_bank_number(self.prg_bank_count(), slot);
        bank * PRG_BANK_LEN + (addr as usize & 0x1FFF)
    }

    fn chr_bank_value(&self, addr: u16) -> u8 {
        let slot = (addr as usize) / CHR_BANK_LEN_1K;
        self.core.effective_chr_bank_value(slot)
    }
}

impl Mapper for Waixing {
    prg_ram_battery!(prg_ram);

    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x5000..=0x5FFF if self.variant == WaixingVariant::TypeE => {
                Some(self.prg_ram[0x2000 + (addr - 0x5000) as usize])
            }
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x5000..=0x5FFF if self.variant == WaixingVariant::TypeE => {
                self.prg_ram[0x2000 + (addr - 0x5000) as usize] = data;
                true
            }
            0x6000..=0x7FFF => {
                if self.core.prg_ram_enabled() && !self.core.prg_ram_write_protect() {
                    self.prg_ram[(addr - 0x6000) as usize] = data;
                }
                true
            }
            0x8000..=0xFFFF => self
                .core
                .write_register(addr, data, Some(&mut self.mirroring)),
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        if self.chr_ram_full_window {
            return Some(self.chr_ram[addr as usize]);
        }
        let bank_value = self.chr_bank_value(addr);
        if let Some(ram_bank) = self.variant.chr_ram_bank(bank_value) {
            let offset = ram_bank * CHR_BANK_LEN_1K + (addr as usize & 0x03FF);
            return Some(self.chr_ram[offset]);
        }
        let bank = (bank_value as usize) % self.chr_bank_count_1k();
        Some(self.chr_rom[bank * CHR_BANK_LEN_1K + (addr as usize & 0x03FF)])
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        if self.chr_ram_full_window {
            self.chr_ram[addr as usize] = data;
            return true;
        }
        let bank_value = self.chr_bank_value(addr);
        if let Some(ram_bank) = self.variant.chr_ram_bank(bank_value) {
            let offset = ram_bank * CHR_BANK_LEN_1K + (addr as usize & 0x03FF);
            self.chr_ram[offset] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        self.core.check_a12(addr, ppu_cycle);
    }

    fn irq_line(&self) -> bool {
        self.core.irq_line()
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        writer.write_bytes(&self.chr_ram);
        writer.write_u8(encode_mirroring(self.mirroring));
        self.core.save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        reader.read_bytes_into(&mut self.chr_ram)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.core.load_state(reader)?;
        Ok(())
    }
}
