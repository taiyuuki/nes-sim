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
    // 逻辑WRAM bank(0-7)到物理8K块(0-7)的映射，255=未连接（open bus）
    wram_index: [u8; 8],
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
    rendering_enabled: bool,
    current_scanline: i16,

    // PPU寄存器监控（$2000/$2001，由PPU通过ppu_register_write通知）
    sprite_8x16: bool, // $2000 bit5
    bg_show: bool,     // $2001 bit3（BG显示）
    mask_subst: bool,  // $2001 bit3|bit4（任一E位即启用MMC5替换功能）

    // 扩展属性模式：最近一次BG tile fetch对应的ExRAM字节
    // 高2位=该tile的palette，低6位=该tile的4K CHR bank
    exattr_byte: u8,

    // Vertical split状态（$5200-$5202）
    split_x: u8,             // 当前扫描线内的BG tile fetch计数（0-31环绕）
    split_y: u8,             // split自己的垂直位置（像素，239环绕）
    split_inside: bool,      // 最近一次BG tile fetch是否位于split区
    split_line_active: bool, // 上一条扫描线是否渲染（用于split_y复位判定）

    // 扫描线检测：在ppu_read_nametable中通过nametable读取触发IRQ
    new_scanline: bool,

    // Audio
    audio: Rc<RefCell<Mmc5Audio>>,
}

impl Mmc5 {
    pub(super) fn new(
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
        _mirroring: Mirroring,
        wram_banks: usize,
        chr_ram_len: usize,
    ) -> Self {
        let chr = if chr_rom.is_empty() {
            // NES 2.0头可声明大于8K的CHR-RAM（如FF2/FF3汉化版为32K，
            // 其分块字库依赖bank 8+所在的物理空间，按8K折返会导致数据互相覆盖）
            ChrMemory::Ram(vec![0; chr_ram_len.max(CHR_BANK_LEN)])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        let audio = Rc::new(RefCell::new(Mmc5Audio::new()));

        Self {
            prg_rom,
            prg_ram: vec![0; WRAM_SIZE],
            wram_index: Self::build_wram_index(wram_banks),
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
            rendering_enabled: false,
            current_scanline: -1,
            sprite_8x16: false,
            bg_show: false,
            mask_subst: false,
            exattr_byte: 0,
            split_x: 0,
            split_y: 0,
            split_inside: false,
            split_line_active: false,
            new_scanline: false,
            audio,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_LEN
    }

    // 逻辑bank寄存器到物理WRAM块的映射，模拟不同卡带的实际WRAM容量：
    // 0块=无WRAM，1块(8K)=bank0-3镜像，2块(16K)=bank4-7映射第二块，
    // 4块(32K)=bank4-7未连接，8块(64K)=完全独立（fceux BuildWRAMSizeTable语义）
    fn build_wram_index(banks: usize) -> [u8; 8] {
        match banks {
            0 => [255; 8],
            1 => [0, 0, 0, 0, 255, 255, 255, 255],
            2 => [0, 0, 0, 0, 1, 1, 1, 1],
            4 => [0, 1, 2, 3, 255, 255, 255, 255],
            _ => [0, 1, 2, 3, 4, 5, 6, 7],
        }
    }

    // 当前$6000-$7FFF窗口对应的物理8K块，None=open bus
    fn current_wram_block(&self) -> Option<usize> {
        match self.wram_index[(self.wram_bank & 7) as usize] {
            255 => None,
            block => Some(block as usize),
        }
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
            _ => {
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
            match self.wram_index[bank_idx & 7] {
                255 => (addr >> 8) as u8, // 未连接的WRAM bank：open bus
                block => self.prg_ram[block as usize * PRG_BANK_LEN + offset],
            }
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
            let block = self.wram_index[bank_idx & 7];
            if block != 255 {
                self.prg_ram[block as usize * PRG_BANK_LEN + offset] = data;
            }
        }
        true
    }

    // CHR read using Mode A banks ($5120-$5127)
    // 注意：寄存器值的单位跟随$5101当前模式的bank大小
    fn chr_index_a(&self, addr: u16) -> usize {
        let slot = (addr as usize) / CHR_BANK_1K;
        let offset = (addr as usize) & 0x03FF;
        let chr_size = self.chr_size();

        match self.chr_mode {
            0 => {
                // 8KB mode: $5127选择8K bank
                let bank = self.chr_banks_a[7] as usize;
                (bank * 0x2000 + addr as usize) % chr_size
            }
            1 => {
                // 4KB mode: $5123→$0000-$0FFF, $5127→$1000-$1FFF
                let bank = if slot < 4 {
                    self.chr_banks_a[3] as usize
                } else {
                    self.chr_banks_a[7] as usize
                };
                (bank * CHR_BANK_1K * 4 + (slot & 3) * CHR_BANK_1K + offset) % chr_size
            }
            2 => {
                // 2KB mode: $5121/$5123/$5125/$5127
                let bank = self.chr_banks_a[slot | 1] as usize;
                (bank * CHR_BANK_1K * 2 + (slot & 1) * CHR_BANK_1K + offset) % chr_size
            }
            _ => {
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
                // 8KB mode: $512B选择8K bank
                let bank = self.chr_banks_b[3] as usize;
                (bank * 0x2000 + addr as usize) % chr_size
            }
            1 => {
                // 4KB mode: $512B同时映射到两个半区
                let bank = self.chr_banks_b[3] as usize;
                (bank * CHR_BANK_1K * 4 + (slot & 3) * CHR_BANK_1K + offset) % chr_size
            }
            2 => {
                // 2KB mode: $5129→$0000/$1000, $512B→$0800/$1800
                let bank = if slot < 4 {
                    self.chr_banks_b[1] as usize
                } else {
                    self.chr_banks_b[3] as usize
                };
                (bank * CHR_BANK_1K * 2 + (slot & 1) * CHR_BANK_1K + offset) % chr_size
            }
            _ => {
                let bank = self.chr_banks_b[slot & 3] as usize;
                (bank * CHR_BANK_1K + offset) % chr_size
            }
        }
    }

    // 渲染期BG/sprite pattern fetch的CHR索引选择
    // 优先级：split区 > 扩展属性模式 > 8x16(B banks) / 8x8(A banks)
    fn chr_fetch_index(&self, addr: u16) -> usize {
        if self.sprite_phase {
            // sprite pattern fetch：始终使用A banks
            return self.chr_index_a(addr);
        }
        if self.rendering_enabled {
            if self.split_inside {
                // split区：固定4K bank（$5202，不受$5101/$5130影响）
                let bank = self.split_bank as usize;
                return (bank * 0x1000 + (addr as usize & 0x0FFF)) % self.chr_size();
            }
            if self.exram_mode == 1 {
                // 扩展属性模式：每tile独立4K bank（ExRAM低6位 + $5130高2位）
                let bank =
                    (self.exattr_byte & 0x3F) as usize | ((self.chr_high_bits as usize & 3) << 6);
                return (bank * 0x1000 + (addr as usize & 0x0FFF)) % self.chr_size();
            }
            if self.sprite_8x16 {
                return self.chr_index_b(addr);
            }
        }
        // 8x8模式BG也用A banks；非渲染期由ab_mode在上层决定
        self.chr_index_a(addr)
    }

    fn split_enabled_now(&self) -> bool {
        // split仅在ExRAM模式%00/%01、BG渲染中生效
        (self.split_control & 0x80) != 0
            && self.exram_mode <= 1
            && self.bg_show
            && self.rendering_enabled
    }

    fn write_chr(&mut self, addr: u16, data: u8) {
        let index = if self.rendering_enabled {
            self.chr_fetch_index(addr)
        } else if self.ab_mode == 0 {
            self.chr_index_a(addr)
        } else {
            self.chr_index_b(addr)
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
    wram_banks: usize,
    chr_ram_len: usize,
) -> (Mmc5, Vec<Box<dyn ExpansionAudioChip>>) {
    let mmc5 = Mmc5::new(prg_rom, chr_rom, mirroring, wram_banks, chr_ram_len);
    let audio_chip = Mmc5AudioChip::new(mmc5.audio.clone());
    (mmc5, vec![Box::new(audio_chip)])
}

// 已知官方MMC5卡带的实际WRAM容量（8K块数），按PRG+CHR数据的CRC32匹配
// （fceux DetectMMC5WRAMSize同表）；未知CRC默认64K
const MMC5_WRAM_TABLE: [(u32, usize); 26] = [
    (0x6F4E4312, 4), // Aoki Ookami to Shiroki Mejika - Genchou Hishi
    (0x15FE6D0F, 2), // Bandit Kings of Ancient China
    (0x671F23A8, 0), // Castlevania III (E)
    (0xCD4E7430, 0), // Castlevania III (KC)
    (0xED2465BE, 0), // Castlevania III (U)
    (0xFE3488D1, 2), // Daikoukai Jidai
    (0x0EC6C023, 1), // Gemfire
    (0x0AFB395E, 0), // Gun Sight
    (0x1CED086F, 2), // Ishin no Arashi
    (0x9CBADC25, 1), // Just Breed
    (0x6396B988, 2), // L'Empereur (J)
    (0x9C18762B, 2), // L'Empereur (U)
    (0xB0480AE9, 0), // Laser Invasion
    (0xB4735FAC, 0), // Metal Slader Glory
    (0xF540677B, 4), // Nobunaga no Yabou - Bushou Fuuun Roku
    (0xEEE9A682, 2), // Nobunaga no Yabou - Sengoku Gunyuu Den (PRG0)
    (0xF9B4240F, 2), // Nobunaga no Yabou - Sengoku Gunyuu Den (PRG1)
    (0x8CE478DB, 2), // Nobunaga's Ambition 2
    (0xF011E490, 4), // Romance of the Three Kingdoms II
    (0xBC80FB52, 1), // Royal Blood
    (0x184C2124, 4), // Sangokushi II (PRG0)
    (0xEE8E6553, 4), // Sangokushi II (PRG1)
    (0xD532E98F, 1), // Shin 4 Nin Uchi Mahjong - Yakuman Tengoku
    (0x39F2CE4B, 2), // Suikoden - Tenmei no Chikai
    (0xBB7F829A, 0), // Uchuu Keibitai SDF
    (0xACA15643, 2), // Uncharted Waters
];

pub(super) fn mmc5_wram_banks(prg_chr_crc32: u32) -> usize {
    for (crc, banks) in MMC5_WRAM_TABLE {
        if crc == prg_chr_crc32 {
            return banks;
        }
    }
    8
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
            0x6000..=0x7FFF => match self.current_wram_block() {
                Some(block) => Some(self.prg_ram[block * PRG_BANK_LEN + (addr - 0x6000) as usize]),
                None => Some((addr >> 8) as u8), // open bus
            },
            0x8000..=0xFFFF => {
                // NMI向量($FFFA/$FFFB)读取：清除in-frame标志、复位扫描线计数器、自动ack IRQ
                // （仅NMI向量；IRQ向量$FFFE读取不能触发，否则每次中断响应都会清帧状态）
                if matches!(addr, 0xFFFA | 0xFFFB) {
                    if self.in_frame {
                        self.in_frame = false;
                        self.irq_counter = 0;
                    }
                    self.irq_pending = false;
                }

                Some(self.read_prg(addr))
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
                // 模式%11时写入被忽略（fceux/nestopia语义；其余模式含ExAttr均允许写入）
                if self.exram_mode != 3 {
                    self.exram[(addr - 0x5C00) as usize] = data;
                }
                true
            }
            0x6000..=0x7FFF => {
                if let Some(block) = self
                    .current_wram_block()
                    .filter(|_| self.wram_write_allowed())
                {
                    self.prg_ram[block * PRG_BANK_LEN + (addr - 0x6000) as usize] = data;
                }
                true
            }
            0x8000..=0xFFFF => self.write_prg(addr, data),
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if addr < 0x2000 {
            let index = if self.rendering_enabled {
                // 渲染期：sprite相位用A banks；BG相位按split/ExAttr/8x16/8x8选择
                self.chr_fetch_index(addr)
            } else if self.ab_mode == 0 {
                // 非渲染期（$2007访问）：使用最后写入的banks组
                self.chr_index_a(addr)
            } else {
                self.chr_index_b(addr)
            };
            Some(match &self.chr {
                ChrMemory::Rom(c) => c[index],
                ChrMemory::Ram(c) => c[index],
            })
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

        let offset = (addr - 0x2000) & 0x0FFF;
        let slot = (offset >> 10) as usize;
        let inner = (offset & 0x03FF) as usize;
        // 仅渲染期的BG fetch参与IRQ计数/split/扩展属性；$2007访问走普通路径
        let bg_fetch = self.rendering_enabled && !self.sprite_phase;

        // MMC5扫描线检测：每条扫描线的第一次BG nametable读取推进IRQ计数器
        if bg_fetch && self.new_scanline {
            self.new_scanline = false;
            if !self.in_frame {
                self.in_frame = true;
                self.irq_counter = 0;
            } else {
                self.irq_counter = self.irq_counter.wrapping_add(1);
                if self.irq_counter == self.irq_scanline_target && self.irq_enabled {
                    self.irq_pending = true;
                }
            }
        }

        if inner < 0x3C0 {
            // tile fetch
            if bg_fetch {
                // 记录该tile的ExRAM字节（扩展属性模式下提供palette+CHR bank）
                self.exattr_byte = self.exram[inner];

                // vertical split：按BG tile fetch计数推进列计数器
                if self.split_enabled_now() {
                    self.split_x = (self.split_x + 1) & 0x1F;
                    let threshold = self.split_control & 0x1F;
                    // bit6=0右侧split，bit6=1左侧split
                    let inside = if self.split_control & 0x40 == 0 {
                        self.split_x >= threshold
                    } else {
                        self.split_x < threshold
                    };
                    self.split_inside = inside;
                    if inside {
                        // split区NT数据来自ExRAM，行由split自己的y决定
                        let tile =
                            ((u16::from(self.split_y) & 0xF8) << 2) | u16::from(self.split_x);
                        return Some(self.exram[tile as usize]);
                    }
                } else {
                    self.split_inside = false;
                }
            }

            match self.nt_source(slot) {
                NtSource::Vram(_) => None,
                NtSource::ExRam => Some(self.exram[inner]),
                NtSource::Fill => Some(self.fill_nt[inner]),
            }
        } else {
            // attribute fetch
            if bg_fetch {
                if self.split_inside {
                    // split区属性：ExRAM属性表($3C0-$3FF)按split tile坐标取象限，
                    // 复制到4个象限以抵消PPU按自己scroll做的象限选择
                    let tile = ((u16::from(self.split_y) & 0xF8) << 2) | u16::from(self.split_x);
                    let attr_addr = (0x3C0 | ((tile >> 4) & 0x38) | ((tile >> 2) & 0x07)) as usize;
                    let shift = ((tile >> 4) & 0x4) | (tile & 0x2);
                    let palette = (self.exram[attr_addr] >> shift) & 0x3;
                    return Some(palette | (palette << 2) | (palette << 4) | (palette << 6));
                }
                if self.exram_mode == 1 {
                    // 扩展属性模式：每tile独立palette（ExRAM高2位），复制到4个象限
                    let palette = self.exattr_byte >> 6;
                    return Some(palette | (palette << 2) | (palette << 4) | (palette << 6));
                }
            }

            match self.nt_source(slot) {
                NtSource::Vram(_) => None,
                NtSource::ExRam => Some(self.exram[inner]),
                NtSource::Fill => Some(self.fill_nt[inner]),
            }
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
        self.irq_pending && self.irq_enabled
    }

    fn notify_scanline(&mut self, scanline: i16, rendering_on: bool) {
        self.rendering_enabled = rendering_on;
        if scanline != self.current_scanline {
            self.current_scanline = scanline;
        }

        // 渲染停止或进入VBlank：清除帧内状态并复位扫描线计数器
        if !rendering_on || scanline >= 241 {
            if self.in_frame {
                self.in_frame = false;
                self.irq_counter = 0;
                self.irq_pending = false;
                self.new_scanline = false;
            }
            self.split_line_active = false;
            return;
        }

        // 可见渲染扫描线开始
        self.new_scanline = true;

        // split列计数器每线复位（首个tile fetch后变为0）
        self.split_x = 0x1F;
        self.split_inside = false;
        // split垂直计数器独立于$5200使能状态持续维护（游戏可能帧中启用split）
        if self.split_line_active {
            // 持续渲染：split垂直位置每线+1，239环绕回0
            self.split_y = if self.split_y < 239 {
                self.split_y + 1
            } else {
                0
            };
        } else {
            // 渲染恢复：split垂直位置复位为$5201
            self.split_y = self.split_scroll.min(239);
        }
        self.split_line_active = true;
    }

    fn set_ppu_sprite_phase(&mut self, sprite_phase: bool) {
        self.sprite_phase = sprite_phase;
    }

    fn ppu_register_write(&mut self, addr: u16, data: u8) {
        match addr {
            // $2000 bit5：sprite尺寸（决定BG fetch用A还是B banks）
            0x2000 => self.sprite_8x16 = (data & 0x20) != 0,
            // $2001 bit3/bit4：BG/sprite显示（E位全清时禁用MMC5全部替换功能）
            0x2001 => {
                self.bg_show = (data & 0x08) != 0;
                self.mask_subst = (data & 0x18) != 0;
            }
            _ => {}
        }
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
        writer.write_bool(self.sprite_phase);
        writer.write_bool(self.rendering_enabled);
        writer.write_i16(self.current_scanline);
        writer.write_bool(self.sprite_8x16);
        writer.write_bool(self.bg_show);
        writer.write_bool(self.mask_subst);
        writer.write_u8(self.exattr_byte);
        writer.write_u8(self.split_x);
        writer.write_u8(self.split_y);
        writer.write_bool(self.split_inside);
        writer.write_bool(self.split_line_active);
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
        self.sprite_phase = reader.read_bool()?;
        self.rendering_enabled = reader.read_bool()?;
        self.current_scanline = reader.read_i16()?;
        self.sprite_8x16 = reader.read_bool()?;
        self.bg_show = reader.read_bool()?;
        self.mask_subst = reader.read_bool()?;
        self.exattr_byte = reader.read_u8()?;
        self.split_x = reader.read_u8()?;
        self.split_y = reader.read_u8()?;
        self.split_inside = reader.read_bool()?;
        self.split_line_active = reader.read_bool()?;
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
