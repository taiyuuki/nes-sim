use std::cell::RefCell;
use std::rc::Rc;

use super::Mapper;
use crate::apu::ExpansionAudioChip;
use crate::cartridge::expansion_audio::mmc5::{Mmc5Audio, Mmc5AudioChip};
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_LEN: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const WRAM_SIZE: usize = 0x10000; // 64KB
const EXRAM_SIZE: usize = 1024;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mmc5 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    exram: Vec<u8>,
    fill_nt: Vec<u8>,

    // Registers
    prg_mode: u8,
    chr_mode: u8,
    wram_write_enable: [u8; 2],
    exram_mode: u8,
    nt_mapping: u8,
    fill_tile: u8,
    fill_attr: u8,
    wram_bank: u8,
    prg_banks: [u8; 4],
    chr_banks_a: [u16; 8],
    chr_banks_b: [u16; 4],
    chr_high_bits: u8,
    ab_mode: u8,
    multiplier: [u8; 2],

    // IRQ
    irq_scanline_target: u8,
    irq_enabled: bool,
    irq_pending: bool,
    in_frame: bool,
    irq_counter: u8,

    // Split mode (registers only, not yet fully implemented)
    split_control: u8,
    split_scroll: u8,
    split_bank: u8,

    // Sprite/background phase tracking
    sprite_phase: bool, // PPU是否在sprite阶段（由PPU通过set_ppu_sprite_phase设置）
    ppu_fetch_count: u8,
    prev_fetch: u16,
    prev_prev_fetch: u16,
    sprite_mode: bool,
    rendering_enabled: bool,
    current_scanline: i16,

    // 扫描线检测：在ppu_read_nametable中通过nametable读取触发IRQ
    new_scanline: bool,

    // Audio
    audio: Rc<RefCell<Mmc5Audio>>,
}

impl Mmc5 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, _mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        let audio = Rc::new(RefCell::new(Mmc5Audio::new()));

        Self {
            prg_rom,
            prg_ram: vec![0; WRAM_SIZE],
            chr,
            exram: vec![0; EXRAM_SIZE],
            fill_nt: vec![0; EXRAM_SIZE],
            prg_mode: 3,
            chr_mode: 3,
            wram_write_enable: [0xFF; 2],
            exram_mode: 0,
            nt_mapping: 0,
            fill_tile: 0,
            fill_attr: 0,
            wram_bank: 0,
            prg_banks: [0xFF; 4],
            chr_banks_a: [0; 8],
            chr_banks_b: [0; 4],
            chr_high_bits: 0,
            ab_mode: 0,
            multiplier: [0; 2],
            irq_scanline_target: 0,
            irq_enabled: false,
            irq_pending: false,
            in_frame: false,
            irq_counter: 0,
            split_control: 0,
            split_scroll: 0,
            split_bank: 0,
            sprite_phase: false,
            ppu_fetch_count: 0,
            prev_fetch: 0,
            prev_prev_fetch: 0,
            sprite_mode: false,
            rendering_enabled: false,
            current_scanline: -1,
            new_scanline: false,
            audio,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_LEN
    }

    fn chr_size(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(c) => c.len(),
            ChrMemory::Ram(c) => c.len(),
        }
    }

    fn wram_write_allowed(&self) -> bool {
        (self.wram_write_enable[0] & 3) == 2 && (self.wram_write_enable[1] & 3) == 1
    }

    // Returns (is_rom, bank_index) for a given PRG slot
    fn prg_slot_bank(&self, slot: usize) -> (bool, usize) {
        match self.prg_mode {
            0 => {
                // 32KB mode: bank3, always ROM
                let base = (self.prg_banks[3] as usize & 0x7F) >> 2;
                (true, base * 4 + slot)
            }
            1 => {
                if slot < 2 {
                    // $8000-$BFFF: bank1 (16KB)
                    let is_rom = self.prg_banks[1] & 0x80 != 0;
                    let base = (self.prg_banks[1] as usize & 0x7E) >> 1;
                    (is_rom, base * 2 + slot)
                } else {
                    // $C000-$FFFF: bank3 (16KB), always ROM
                    let base = (self.prg_banks[3] as usize & 0x7F) >> 1;
                    (true, base * 2 + (slot - 2))
                }
            }
            2 => {
                if slot < 2 {
                    // $8000-$BFFF: bank1 (16KB)
                    let is_rom = self.prg_banks[1] & 0x80 != 0;
                    let base = self.prg_banks[1] as usize & 0x7E;
                    (is_rom, base + slot)
                } else if slot == 2 {
                    // $C000-$DFFF: bank2 (8KB)
                    let is_rom = self.prg_banks[2] & 0x80 != 0;
                    (is_rom, self.prg_banks[2] as usize & 0x7F)
                } else {
                    // $E000-$FFFF: bank3 (8KB), always ROM
                    (true, self.prg_banks[3] as usize & 0x7F)
                }
            }
            3 | _ => {
                if slot < 3 {
                    let is_rom = self.prg_banks[slot] & 0x80 != 0;
                    (is_rom, self.prg_banks[slot] as usize & 0x7F)
                } else {
                    (true, self.prg_banks[3] as usize & 0x7F)
                }
            }
        }
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let slot = ((addr - 0x8000) as usize) / PRG_BANK_LEN;
        let offset = (addr as usize) & 0x1FFF;
        let (is_rom, bank_idx) = self.prg_slot_bank(slot);

        if is_rom {
            let bank_count = self.prg_bank_count();
            let bank = bank_idx % bank_count;
            self.prg_rom[bank * PRG_BANK_LEN + offset]
        } else {
            let ram_bank = bank_idx & 7; // 8 possible 8KB banks in 64KB
            self.prg_ram[ram_bank * PRG_BANK_LEN + offset]
        }
    }

    fn write_prg(&mut self, addr: u16, data: u8) -> bool {
        if !self.wram_write_allowed() {
            return true;
        }
        let slot = ((addr - 0x8000) as usize) / PRG_BANK_LEN;
        let offset = (addr as usize) & 0x1FFF;
        let (is_rom, bank_idx) = self.prg_slot_bank(slot);

        if !is_rom {
            let ram_bank = bank_idx & 7;
            self.prg_ram[ram_bank * PRG_BANK_LEN + offset] = data;
        }
        true
    }

    // CHR read using Mode A banks (sprite banks)
    fn chr_index_a(&self, addr: u16) -> usize {
        let slot = (addr as usize) / CHR_BANK_1K;
        let offset = (addr as usize) & 0x03FF;
        let chr_size = self.chr_size();

        match self.chr_mode {
            0 => {
                let bank = self.chr_banks_a[7] as usize;
                (bank * CHR_BANK_1K + (addr as usize)) % chr_size
            }
            1 => {
                let bank = if slot < 4 {
                    self.chr_banks_a[3] as usize
                } else {
                    self.chr_banks_a[7] as usize
                };
                (bank * CHR_BANK_1K * 4 + (slot & 3) * CHR_BANK_1K + offset) % chr_size
            }
            2 => {
                let bank = self.chr_banks_a[slot | 1] as usize;
                (bank * CHR_BANK_1K * 2 + (slot & 1) * CHR_BANK_1K + offset) % chr_size
            }
            3 | _ => {
                let bank = self.chr_banks_a[slot] as usize;
                (bank * CHR_BANK_1K + offset) % chr_size
            }
        }
    }

    fn chr_index_b(&self, addr: u16) -> usize {
        let slot = (addr as usize) / CHR_BANK_1K;
        let offset = (addr as usize) & 0x03FF;
        let chr_size = self.chr_size();

        match self.chr_mode {
            0 => {
                let bank = self.chr_banks_b[3] as usize;
                (bank * CHR_BANK_1K + (addr as usize)) % chr_size
            }
            1 => {
                let bank = self.chr_banks_b[3] as usize;
                (bank * CHR_BANK_1K * 4 + (slot & 3) * CHR_BANK_1K + offset) % chr_size
            }
            2 => {
                let bank = if slot < 4 {
                    self.chr_banks_b[1] as usize
                } else {
                    self.chr_banks_b[3] as usize
                };
                (bank * CHR_BANK_1K * 2 + (slot & 1) * CHR_BANK_1K + offset) % chr_size
            }
            3 | _ => {
                let bank = self.chr_banks_b[slot & 3] as usize;
                (bank * CHR_BANK_1K + offset) % chr_size
            }
        }
    }

    fn should_use_background_banks(&self) -> bool {
        // 背景阶段用B banks，sprite阶段用A banks
        !self.sprite_phase
    }

    fn write_chr(&mut self, addr: u16, data: u8) {
        let use_bg_banks = self.should_use_background_banks();
        let index = if self.sprite_mode {
            self.chr_index_a(addr)
        } else if use_bg_banks {
            self.chr_index_b(addr)
        } else {
            self.chr_index_a(addr)
        };
        match &mut self.chr {
            ChrMemory::Ram(c) => c[index] = data,
            ChrMemory::Rom(_) => {}
        }
    }

    fn update_fill_buffer(&mut self) {
        let tile_byte = self.fill_tile;
        let attr_byte = self.fill_attr;
        let attr_expanded = attr_byte | (attr_byte << 2) | (attr_byte << 4) | (attr_byte << 6);

        for i in 0..960 {
            self.fill_nt[i] = tile_byte;
        }
        for i in 960..EXRAM_SIZE {
            self.fill_nt[i] = attr_expanded;
        }
    }

    fn nt_source(&self, slot: usize) -> NtSource {
        match (self.nt_mapping >> (slot * 2)) & 3 {
            0 => NtSource::Vram(0),
            1 => NtSource::Vram(0x400),
            2 => NtSource::ExRam,
            3 => NtSource::Fill,
            _ => unreachable!(),
        }
    }
}

enum NtSource {
    Vram(usize),
    ExRam,
    Fill,
}

pub(super) fn new_mmc5(
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,
) -> (Mmc5, Vec<Box<dyn ExpansionAudioChip>>) {
    let mmc5 = Mmc5::new(prg_rom, chr_rom, mirroring);
    let audio_chip = Mmc5AudioChip::new(mmc5.audio.clone());
    (mmc5, vec![Box::new(audio_chip)])
}

impl Mapper for Mmc5 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x5000..=0x5014 => Some(0),
            0x5015 => Some(self.audio.borrow().status()),
            0x5100..=0x5FFF => match addr {
                0x5204 => {
                    let status = (if self.irq_pending { 0x80 } else { 0 })
                        | (if self.in_frame { 0x40 } else { 0 });
                    self.irq_pending = false;
                    Some(status)
                }
                0x5205 => Some((self.multiplier[0] as u16 * self.multiplier[1] as u16) as u8),
                0x5206 => {
                    Some(((self.multiplier[0] as u16 * self.multiplier[1] as u16) >> 8) as u8)
                }
                0x5C00..=0x5FFF => Some(self.exram[(addr - 0x5C00) as usize]),
                _ => Some(0),
            },
            0x6000..=0x7FFF => {
                let ram_addr =
                    (self.wram_bank as usize & 7) * PRG_BANK_LEN + (addr - 0x6000) as usize;
                Some(self.prg_ram[ram_addr])
            }
            0x8000..=0xFFFF => {
                let val = self.read_prg(addr);

                Some(val)
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x5000..=0x5015 => {
                self.audio.borrow_mut().write(addr, data);
                true
            }
            0x5100 => {
                self.prg_mode = data & 3;
                true
            }
            0x5101 => {
                self.chr_mode = data & 3;
                true
            }
            0x5102 => {
                self.wram_write_enable[0] = data;
                true
            }
            0x5103 => {
                self.wram_write_enable[1] = data;
                true
            }
            0x5104 => {
                self.exram_mode = data & 3;
                true
            }
            0x5105 => {
                self.nt_mapping = data;
                true
            }
            0x5106 => {
                self.fill_tile = data;
                self.update_fill_buffer();
                true
            }
            0x5107 => {
                self.fill_attr = data & 3;
                self.update_fill_buffer();
                true
            }
            0x5113 => {
                self.wram_bank = data;
                true
            }
            0x5114..=0x5117 => {
                self.prg_banks[(addr - 0x5114) as usize] = data;
                true
            }
            0x5120..=0x5127 => {
                let idx = (addr - 0x5120) as usize;
                self.chr_banks_a[idx] = u16::from(data) | (u16::from(self.chr_high_bits & 3) << 8);
                self.ab_mode = 0;
                true
            }
            0x5128..=0x512B => {
                let idx = (addr - 0x5128) as usize;
                self.chr_banks_b[idx] = u16::from(data) | (u16::from(self.chr_high_bits & 3) << 8);
                self.ab_mode = 1;
                true
            }
            0x5130 => {
                self.chr_high_bits = data;
                true
            }
            0x5200 => {
                self.split_control = data;
                true
            }
            0x5201 => {
                self.split_scroll = data;
                true
            }
            0x5202 => {
                self.split_bank = data;
                true
            }
            0x5203 => {
                self.irq_scanline_target = data;
                true
            }
            0x5204 => {
                self.irq_enabled = (data & 0x80) != 0;
                true
            }
            0x5205 => {
                self.multiplier[0] = data;
                true
            }
            0x5206 => {
                self.multiplier[1] = data;
                true
            }
            0x5C00..=0x5FFF => {
                if self.exram_mode != 3 {
                    self.exram[(addr - 0x5C00) as usize] = data;
                }
                true
            }
            0x6000..=0x7FFF => {
                if self.wram_write_allowed() {
                    let ram_addr =
                        (self.wram_bank as usize & 7) * PRG_BANK_LEN + (addr - 0x6000) as usize;
                    self.prg_ram[ram_addr] = data;
                }
                true
            }
            0x8000..=0xFFFF => self.write_prg(addr, data),
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if addr < 0x2000 {
            let index = if self.sprite_phase {
                // 渲染中sprite阶段：使用A banks ($5120-$5127)
                self.chr_index_a(addr)
            } else if self.rendering_enabled {
                // 渲染中背景阶段：使用B banks ($5128-$512B)
                self.chr_index_b(addr)
            } else {
                // 非渲染期（$2007访问）：使用最后写入的banks组
                if self.ab_mode == 0 {
                    self.chr_index_a(addr)
                } else {
                    self.chr_index_b(addr)
                }
            };
            return Some(match &self.chr {
                ChrMemory::Rom(c) => c[index],
                ChrMemory::Ram(c) => c[index],
            });
        } else {
            None
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if addr < 0x2000 {
            self.write_chr(addr, data);
            true
        } else {
            false
        }
    }

    fn mirroring(&self) -> Mirroring {
        Mirroring::Vertical // MMC5 controls mirroring via nt_mapping
    }

    fn map_nametable_addr(&self, addr: u16) -> Option<usize> {
        if !(0x2000..=0x3EFF).contains(&addr) {
            return None;
        }
        let offset = (addr - 0x2000) & 0x0FFF;
        let slot = (offset >> 10) as usize;
        let inner = (offset & 0x03FF) as usize;

        match self.nt_source(slot) {
            NtSource::Vram(base) => Some(base + inner),
            NtSource::ExRam | NtSource::Fill => None, // Handled by ppu_read_nametable
        }
    }

    fn ppu_read_nametable(&mut self, addr: u16) -> Option<u8> {
        if !(0x2000..=0x3EFF).contains(&addr) {
            return None;
        }

        // MMC5扫描线检测：在每条扫描线的第一条nametable读取时更新IRQ计数器
        // 硬件通过检测PPU读取模式在PPU cycle 4附近触发，此处近似于扫描线开始
        if self.new_scanline {
            self.new_scanline = false;
            if !self.in_frame {
                self.in_frame = true;
                self.irq_counter = 0;
            } else {
                self.irq_counter = self.irq_counter.wrapping_add(1);
                if self.irq_counter == self.irq_scanline_target {
                    self.irq_pending = true;
                }
            }
        }

        // 每次nametable读取时重置CHR读取计数
        // 这样每个tile的CHR读取从0开始计数
        self.ppu_fetch_count = 0;

        self.prev_prev_fetch = self.prev_fetch;
        self.prev_fetch = addr;

        let offset = (addr - 0x2000) & 0x0FFF;
        let slot = (offset >> 10) as usize;
        let inner = (offset & 0x03FF) as usize;

        // 简化：只处理基本的nametable映射，不处理ExCHR
        match self.nt_source(slot) {
            NtSource::Vram(_) => None,
            NtSource::ExRam => Some(self.exram[inner]),
            NtSource::Fill => Some(self.fill_nt[inner]),
        }
    }

    fn ppu_write_nametable(&mut self, addr: u16, data: u8) -> bool {
        if !(0x2000..=0x3EFF).contains(&addr) {
            return false;
        }
        let offset = (addr - 0x2000) & 0x0FFF;
        let slot = (offset >> 10) as usize;
        let inner = (offset & 0x03FF) as usize;

        match self.nt_source(slot) {
            NtSource::Vram(_) => false, // Let default VRAM handle it
            NtSource::ExRam => {
                self.exram[inner] = data;
                true
            }
            NtSource::Fill => true, // Fill mode is read-only
        }
    }

    fn irq_line(&self) -> bool {
        self.irq_pending
    }

    fn notify_scanline(&mut self, scanline: i16, rendering_on: bool) {
        // 更新渲染状态
        self.rendering_enabled = rendering_on;

        // 检测新扫描线
        if scanline != self.current_scanline {
            self.current_scanline = scanline;
            // 新扫描线开始：重置为背景模式
            if scanline >= 0 && scanline < 240 && rendering_on {
                self.sprite_mode = false;
                self.ppu_fetch_count = 0;
            }
        }

        // 当渲染停止或进入VBlank时，清除帧内状态
        if !rendering_on || scanline >= 241 {
            if self.in_frame {
                self.in_frame = false;
                self.irq_counter = 0;
                self.irq_pending = false;
                self.new_scanline = false;
            }
            return;
        }

        // 标记新扫描线，IRQ计数将在ppu_read_nametable中通过nametable读取触发
        self.new_scanline = true;
    }

    fn set_ppu_sprite_phase(&mut self, sprite_phase: bool) {
        if sprite_phase && !self.sprite_phase {
            // 进入sprite阶段：重置fc
            self.ppu_fetch_count = 0;
            self.sprite_mode = false;
        }
        self.sprite_phase = sprite_phase;
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        writer.write_bytes(&self.exram);
        writer.write_bytes(&self.fill_nt);
        writer.write_u8(self.prg_mode);
        writer.write_u8(self.chr_mode);
        writer.write_bytes(&self.wram_write_enable);
        writer.write_u8(self.exram_mode);
        writer.write_u8(self.nt_mapping);
        writer.write_u8(self.fill_tile);
        writer.write_u8(self.fill_attr);
        writer.write_u8(self.wram_bank);
        writer.write_bytes(&self.prg_banks);
        for bank in &self.chr_banks_a {
            writer.write_u16(*bank);
        }
        for bank in &self.chr_banks_b {
            writer.write_u16(*bank);
        }
        writer.write_u8(self.chr_high_bits);
        writer.write_u8(self.ab_mode);
        writer.write_bytes(&self.multiplier);
        writer.write_u8(self.irq_scanline_target);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_pending);
        writer.write_bool(self.in_frame);
        writer.write_u8(self.irq_counter);
        writer.write_u8(self.split_control);
        writer.write_u8(self.split_scroll);
        writer.write_u8(self.split_bank);
        writer.write_u16(self.prev_fetch);
        writer.write_u8(0u8); // exchr_latch removed
        writer.write_u16(0u16); // exchr_cached_bank removed
        writer.write_u8(self.ppu_fetch_count);
        writer.write_u16(self.prev_fetch);
        writer.write_u16(self.prev_prev_fetch);
        writer.write_bool(self.sprite_mode);
        writer.write_bool(self.new_scanline);
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
        reader.read_bytes_into(&mut self.exram)?;
        reader.read_bytes_into(&mut self.fill_nt)?;
        self.prg_mode = reader.read_u8()?;
        self.chr_mode = reader.read_u8()?;
        reader.read_bytes_into(&mut self.wram_write_enable)?;
        self.exram_mode = reader.read_u8()?;
        self.nt_mapping = reader.read_u8()?;
        self.fill_tile = reader.read_u8()?;
        self.fill_attr = reader.read_u8()?;
        self.wram_bank = reader.read_u8()?;
        reader.read_bytes_into(&mut self.prg_banks)?;
        for bank in &mut self.chr_banks_a {
            *bank = reader.read_u16()?;
        }
        for bank in &mut self.chr_banks_b {
            *bank = reader.read_u16()?;
        }
        self.chr_high_bits = reader.read_u8()?;
        self.ab_mode = reader.read_u8()?;
        reader.read_bytes_into(&mut self.multiplier)?;
        self.irq_scanline_target = reader.read_u8()?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_pending = reader.read_bool()?;
        self.in_frame = reader.read_bool()?;
        self.irq_counter = reader.read_u8()?;
        self.split_control = reader.read_u8()?;
        self.split_scroll = reader.read_u8()?;
        self.split_bank = reader.read_u8()?;
        self.prev_fetch = reader.read_u16()?;
        let _ = reader.read_u8()?; // exchr_latch removed
        let _ = reader.read_u16()?; // exchr_cached_bank removed
        self.ppu_fetch_count = reader.read_u8()?;
        self.prev_fetch = reader.read_u16()?;
        self.prev_prev_fetch = reader.read_u16()?;
        self.sprite_mode = reader.read_bool()?;
        self.new_scanline = reader.read_bool()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for MMC5 save state",
                ));
            }
        }
        Ok(())
    }
}
