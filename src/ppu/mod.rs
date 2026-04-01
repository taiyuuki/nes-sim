use crate::cartridge::TVSystem;

pub const FRAME_WIDTH: usize = 256;
pub const FRAME_HEIGHT: usize = 240;
const VISIBLE_FRAME_PIXELS: usize = FRAME_WIDTH * FRAME_HEIGHT;

const STATUS_SPRITE_OVERFLOW: u8 = 0x20;
const STATUS_SPRITE_ZERO_HIT: u8 = 0x40;
const STATUS_VBLANK: u8 = 0x80;
const MASK_GRAYSCALE: u8 = 0x01;
const MASK_SHOW_BG_LEFTMOST: u8 = 0x02;
const MASK_SHOW_SPRITES_LEFTMOST: u8 = 0x04;
const MASK_SHOW_BG: u8 = 0x08;
const MASK_SHOW_SPRITES: u8 = 0x10;
const CTRL_SPRITE_TABLE: u8 = 0x08;
const CTRL_BG_TABLE: u8 = 0x10;
const CTRL_VRAM_INCREMENT: u8 = 0x04;
const CTRL_SPRITE_SIZE: u8 = 0x20;
const CTRL_NMI_ENABLE: u8 = 0x80;
const DOTS_PER_SCANLINE: u16 = 341;
const VISIBLE_SCANLINES: i16 = 240;
const NTSC_CPU_SCHEDULE: [u8; 1] = [3];
const PAL_CPU_SCHEDULE: [u8; 5] = [3, 3, 3, 3, 4];
const DENDY_CPU_SCHEDULE: [u8; 1] = [3];

const PAL: [u8; 32] = [
    0x0F, 0x01, 0x00, 0x01, 0x00, 0x02, 0x02, 0x0D, 0x08, 0x10, 0x08, 0x24, 0x00, 0x00, 0x04, 0x2C,
    0x0F, 0x01, 0x34, 0x03, 0x00, 0x04, 0x00, 0x14, 0x08, 0x3A, 0x00, 0x02, 0x00, 0x20, 0x2C, 0x08,
];

const NES_RGB_PALETTE: [[u8; 3]; 64] = [
    [84, 84, 84],
    [0, 30, 116],
    [8, 16, 144],
    [48, 0, 136],
    [68, 0, 100],
    [92, 0, 48],
    [84, 4, 0],
    [60, 24, 0],
    [32, 42, 0],
    [8, 58, 0],
    [0, 64, 0],
    [0, 60, 0],
    [0, 50, 60],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
    [152, 150, 152],
    [8, 76, 196],
    [48, 50, 236],
    [92, 30, 228],
    [136, 20, 176],
    [160, 20, 100],
    [152, 34, 32],
    [120, 60, 0],
    [84, 90, 0],
    [40, 114, 0],
    [8, 124, 0],
    [0, 118, 40],
    [0, 102, 120],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
    [236, 238, 236],
    [76, 154, 236],
    [120, 124, 236],
    [176, 98, 236],
    [228, 84, 236],
    [236, 88, 180],
    [236, 106, 100],
    [212, 136, 32],
    [160, 170, 0],
    [116, 196, 0],
    [76, 208, 32],
    [56, 204, 108],
    [56, 180, 204],
    [60, 60, 60],
    [0, 0, 0],
    [0, 0, 0],
    [236, 238, 236],
    [168, 204, 236],
    [188, 188, 236],
    [212, 178, 236],
    [236, 174, 236],
    [236, 174, 212],
    [236, 180, 176],
    [228, 196, 144],
    [204, 210, 120],
    [180, 222, 120],
    [168, 226, 144],
    [152, 226, 180],
    [160, 214, 228],
    [160, 162, 160],
    [0, 0, 0],
    [0, 0, 0],
];

pub trait PPUBus {
    fn ppu_read(&mut self, addr: u16) -> u8;
    fn ppu_write(&mut self, addr: u16, data: u8);
    fn check_a12(&mut self, _addr: u16) {}
}

#[derive(Clone, Copy, Default)]
struct SpriteRenderData {
    tile_id: u8,
    row: u8,
    x: u8,
    attributes: u8,
    pattern_lo: u8,
    pattern_hi: u8,
    sprite_zero: bool,
}

pub struct PPU {
    scanline: i16,
    cycles: u16,
    frame: u64,

    oam: [u8; 256],
    oam_addr: u8,

    ctrl: u8,
    mask: u8,
    status: u8,
    open_bus: u8,
    vram_addr: u16,
    temp_vram_addr: u16,
    fine_x: u8,
    write_latch: bool,
    read_buffer: u8,
    odd_frame: bool,
    next_tile_id: u8,
    next_tile_attr: u8,
    next_tile_lsb: u8,
    next_tile_msb: u8,
    bg_pattern_shift_lo: u16,
    bg_pattern_shift_hi: u16,
    bg_attr_shift_lo: u16,
    bg_attr_shift_hi: u16,

    // parameters
    tv_system: TVSystem,
    num_scanlines: i16,
    vblank_lines: i16,

    loopy_v: u16,
    loopy_t: u16,
    bit_map: [u8; 0xF000],
    bg_colors: [u8; 0x100],
    bg_pixels: [u8; 0x100],
    scanline_sprites: [SpriteRenderData; 8],
    scanline_sprite_count: u8,
    suppress_vblank: bool,

    even: bool,
}

impl PPU {
    pub fn new() -> Self {
        Self {
            oam: [0; 256],
            oam_addr: 0,
            frame: 0,
            ctrl: 0,
            mask: 0,
            status: 0,
            open_bus: 0,
            vram_addr: 0,
            temp_vram_addr: 0,
            fine_x: 0,
            write_latch: false,
            read_buffer: 0,
            scanline: 261,
            cycles: 0,
            odd_frame: false,
            next_tile_id: 0,
            next_tile_attr: 0,
            next_tile_lsb: 0,
            next_tile_msb: 0,
            bg_pattern_shift_lo: 0,
            bg_pattern_shift_hi: 0,
            bg_attr_shift_lo: 0,
            bg_attr_shift_hi: 0,
            tv_system: TVSystem::NTSC,
            num_scanlines: 262,
            vblank_lines: 241,

            loopy_v: 0,
            loopy_t: 0,
            bit_map: [0; 0xF000],
            bg_colors: [0; 0x100],
            bg_pixels: [0; 0x100],
            scanline_sprites: [SpriteRenderData::default(); 8],
            scanline_sprite_count: 0,
            suppress_vblank: false,

            even: false,
        }
    }

    pub fn reset(&mut self) {
        self.ctrl = 0;
        self.mask = 0;
        self.status &= !(STATUS_SPRITE_OVERFLOW | STATUS_SPRITE_ZERO_HIT | STATUS_VBLANK);
        self.open_bus = 0;
        self.vram_addr = 0;
        self.temp_vram_addr = 0;
        self.fine_x = 0;
        self.write_latch = false;
        self.read_buffer = 0;
        self.scanline = 261;
        self.cycles = 0;
        self.frame = 0;
        self.odd_frame = false;
        self.next_tile_id = 0;
        self.next_tile_attr = 0;
        self.next_tile_lsb = 0;
        self.next_tile_msb = 0;
        self.bg_pattern_shift_lo = 0;
        self.bg_pattern_shift_hi = 0;
        self.bg_attr_shift_lo = 0;
        self.bg_attr_shift_hi = 0;
        self.set_current_vram_addr(0);
        self.set_temp_vram_addr(0);
        self.bg_pixels = [0; 0x100];
        self.scanline_sprites = [SpriteRenderData::default(); 8];
        self.scanline_sprite_count = 0;
        self.suppress_vblank = false;
    }

    pub fn set_parameters(&mut self, tv_system: TVSystem) {
        self.tv_system = tv_system;
        match tv_system {
            TVSystem::NTSC => {
                self.num_scanlines = 262;
                self.vblank_lines = 241;
            }
            TVSystem::PAL => {
                self.num_scanlines = 312;
                self.vblank_lines = 241;
            }
            TVSystem::DENDY => {
                self.num_scanlines = 312;
                self.vblank_lines = 241;
            }
        }
    }

    pub fn cpu_read_register(&mut self, bus: &mut impl PPUBus, addr: u16) -> u8 {
        match addr {
            0x2002 => self.read_status(),
            0x2004 => {
                let data = self.read_oam_data();
                self.open_bus = data;
                data
            }
            0x2007 => self.read_data(bus),
            _ => self.open_bus,
        }
    }

    pub fn cpu_write_register(&mut self, bus: &mut impl PPUBus, addr: u16, data: u8) {
        self.open_bus = data;

        match addr {
            0x2000 => {
                self.ctrl = data;
                self.set_temp_vram_addr(
                    (self.temp_vram_addr & !0x0C00) | (((data as u16) & 0x03) << 10),
                );
            }
            0x2001 => self.mask = data,
            0x2003 => self.oam_addr = data,
            0x2004 => self.write_oam_data(data),
            0x2005 => self.write_scroll(data),
            0x2006 => self.write_addr(data),
            0x2007 => self.write_data(bus, data),
            _ => {}
        }
    }

    pub fn clock(&mut self, bus: &mut impl PPUBus) {
        let visible_scanline = self.scanline < VISIBLE_SCANLINES;
        let pre_render_scanline = self.scanline == self.num_scanlines - 1;
        let render_scanline = visible_scanline || pre_render_scanline;
        let visible_cycle = self.cycles < 256;
        let fetch_cycle = visible_cycle || (320..337).contains(&self.cycles);

        if self.scanline == self.vblank_lines && self.cycles == 1 && !self.suppress_vblank {
            self.status |= STATUS_VBLANK;
        }

        if self.scanline == self.num_scanlines - 1 && self.cycles == 1 {
            self.status &= !(STATUS_SPRITE_OVERFLOW | STATUS_SPRITE_ZERO_HIT | STATUS_VBLANK);
            self.suppress_vblank = false;
        }

        if render_scanline && self.rendering_on() {
            if visible_scanline && visible_cycle {
                self.draw_bg_pixel(self.cycles as i16, bus);
                self.draw_sprite_pixel(self.cycles as i16, bus);
            }

            if self.bg_on() && fetch_cycle {
                self.update_bg_shifters();
            }

            if self.bg_on() && fetch_cycle {
                self.fetch_bg(bus);
            }

            match self.cycles {
                255 => self.increment_y(),
                256 => {
                    self.load_bg_shifters();
                    self.transfer_x();
                    if self.rendering_on() && (visible_scanline || pre_render_scanline) {
                        self.eval_sprites(bus);
                    }
                }
                279..=303 if pre_render_scanline => self.transfer_y(),
                _ => {}
            }

            if (257..321).contains(&self.cycles) {
                self.fetch_sprite_data(bus);
            }
        }

        if self.should_skip_odd_frame_cycle(pre_render_scanline) {
            self.start_next_frame();
            return;
        }

        self.cycles += 1;
        if self.cycles >= DOTS_PER_SCANLINE {
            self.cycles = 0;
            self.scanline += 1;

            if self.scanline >= self.num_scanlines {
                self.start_next_frame();
            }
        }
    }

    pub fn bg_on(&self) -> bool {
        (self.mask & MASK_SHOW_BG) != 0
    }

    pub fn sprites_on(&self) -> bool {
        (self.mask & MASK_SHOW_SPRITES) != 0
    }

    pub fn rendering_on(&self) -> bool {
        self.bg_on() || self.sprites_on()
    }

    pub fn nmi_line(&self) -> bool {
        (self.ctrl & CTRL_NMI_ENABLE) != 0 && (self.status & STATUS_VBLANK) != 0
    }

    pub fn in_vblank(&self) -> bool {
        (self.status & STATUS_VBLANK) != 0
    }

    pub fn scanline(&self) -> i16 {
        self.scanline
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn frame_pixels(&self) -> &[u8] {
        &self.bit_map[..VISIBLE_FRAME_PIXELS]
    }

    pub fn frame_rgb(&self) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(VISIBLE_FRAME_PIXELS * 3);
        for &pixel in self.frame_pixels() {
            rgb.extend_from_slice(&palette_index_to_rgb(pixel));
        }
        rgb
    }

    pub fn cpu_schedule(&self) -> &'static [u8] {
        match self.tv_system {
            TVSystem::NTSC => &NTSC_CPU_SCHEDULE,
            TVSystem::PAL => &PAL_CPU_SCHEDULE,
            TVSystem::DENDY => &DENDY_CPU_SCHEDULE,
        }
    }

    pub(crate) fn write_oam_dma(&mut self, data: u8) {
        self.write_oam_data(data);
    }

    pub fn oam_byte(&self, index: u8) -> u8 {
        self.oam[index as usize]
    }

    pub fn oam_addr(&self) -> u8 {
        self.oam_addr
    }

    fn read_status(&mut self) -> u8 {
        let mut status_bits = self.status;
        if self.scanline == self.vblank_lines {
            match self.cycles {
                0 => self.suppress_vblank = true,
                1 | 2 => {
                    status_bits |= STATUS_VBLANK;
                    self.suppress_vblank = true;
                }
                _ => {}
            }
        }

        let status = (status_bits & 0xE0) | (self.open_bus & 0x1F);
        self.status &= !STATUS_VBLANK;
        self.write_latch = false;
        self.open_bus = status;
        status
    }

    fn read_data(&mut self, bus: &mut impl PPUBus) -> u8 {
        let addr = self.loopy_v & 0x3FFF;
        let data = if addr >= 0x3F00 {
            self.read_buffer = self.ppu_read_bus(bus, addr.wrapping_sub(0x1000));
            let palette_data = self.ppu_read_bus(bus, addr);
            self.mask_palette_color(palette_data)
        } else {
            let buffered = self.read_buffer;
            self.read_buffer = self.ppu_read_bus(bus, addr);
            buffered
        };

        self.increment_data_access_vram_addr();
        self.open_bus = data;
        data
    }

    fn write_data(&mut self, bus: &mut impl PPUBus, data: u8) {
        let addr = self.loopy_v & 0x3FFF;
        self.ppu_write_bus(bus, addr, data);
        self.increment_data_access_vram_addr();
    }

    fn write_scroll(&mut self, data: u8) {
        if !self.write_latch {
            self.fine_x = data & 0x07;
            self.set_temp_vram_addr((self.temp_vram_addr & !0x001F) | ((data as u16) >> 3));
            self.write_latch = true;
        } else {
            self.set_temp_vram_addr(
                (self.temp_vram_addr & !0x73E0)
                    | ((((data as u16) >> 3) & 0x1F) << 5)
                    | (((data as u16) & 0x07) << 12),
            );
            self.write_latch = false;
        }
    }

    fn write_addr(&mut self, data: u8) {
        if !self.write_latch {
            self.set_temp_vram_addr((self.temp_vram_addr & 0x00FF) | (((data as u16) & 0x3F) << 8));
            self.write_latch = true;
        } else {
            self.set_temp_vram_addr((self.temp_vram_addr & 0x7F00) | data as u16);
            self.set_current_vram_addr(self.temp_vram_addr);
            self.write_latch = false;
        }
    }

    fn increment_vram_addr(&mut self) {
        let increment = if (self.ctrl & CTRL_VRAM_INCREMENT) != 0 {
            32
        } else {
            1
        };
        self.set_current_vram_addr(self.loopy_v.wrapping_add(increment));
    }

    fn increment_data_access_vram_addr(&mut self) {
        if self.rendering_vram_access_active() {
            self.increment_x();
            self.increment_y();
        } else {
            self.increment_vram_addr();
        }
    }

    fn write_oam_data(&mut self, data: u8) {
        if self.rendering_oam_access_active() {
            self.oam_addr = self.oam_addr.wrapping_add(4);
            return;
        }

        self.oam[self.oam_addr as usize] = data;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    fn read_oam_data(&self) -> u8 {
        if self.rendering_oam_clear_phase() {
            return 0xFF;
        }

        let data = self.oam[self.oam_addr as usize];
        if (self.oam_addr & 0x03) == 0x02 {
            data & 0xE3
        } else {
            data
        }
    }

    fn fetch_bg(&mut self, bus: &mut impl PPUBus) {
        match self.cycles & 0x07 {
            0 => {
                self.load_bg_shifters();
                self.fetch_nt(bus);
            }
            2 => {
                let addr = 0x23C0
                    | (self.loopy_v & 0x0C00)
                    | ((self.loopy_v >> 4) & 0x38)
                    | ((self.loopy_v >> 2) & 0x07);
                let attr = self.ppu_read_bus(bus, addr);
                let shift = ((self.loopy_v >> 4) & 0x04) | (self.loopy_v & 0x02);
                self.next_tile_attr = (attr >> shift) as u8 & 0x03;
            }
            4 => {
                let addr = self.bg_pattern_addr(self.next_tile_id);
                self.next_tile_lsb = self.ppu_read_bus(bus, addr);
            }
            6 => {
                let addr = self.bg_pattern_addr(self.next_tile_id).wrapping_add(8);
                self.next_tile_msb = self.ppu_read_bus(bus, addr);
            }
            7 => self.increment_x(),
            _ => {}
        }
    }

    fn fetch_nt(&mut self, bus: &mut impl PPUBus) {
        let addr = 0x2000 | (self.loopy_v & 0x0FFF);
        self.next_tile_id = self.ppu_read_bus(bus, addr);
    }

    fn eval_sprites(&mut self, _bus: &mut impl PPUBus) {
        let target_scanline = if self.scanline == self.num_scanlines - 1 {
            0
        } else {
            (self.scanline as u8).wrapping_add(1)
        };

        self.scanline_sprites = [SpriteRenderData::default(); 8];
        self.scanline_sprite_count = 0;

        let sprite_height = self.sprite_height();
        let mut overflow_start = None;
        for index in 0..64 {
            let row = match self.sprite_row_for_scanline(index, target_scanline, sprite_height) {
                Some(row) => row,
                None => continue,
            };

            if self.scanline_sprite_count >= 8 {
                overflow_start = Some(index);
                break;
            }

            let base = index * 4;
            let tile = self.oam[base + 1];
            let attributes = self.oam[base + 2];
            let x = self.oam[base + 3];

            self.scanline_sprites[self.scanline_sprite_count as usize] = SpriteRenderData {
                tile_id: tile,
                row,
                x,
                attributes,
                pattern_lo: 0,
                pattern_hi: 0,
                sprite_zero: index == 0,
            };
            self.scanline_sprite_count += 1;

            if self.scanline_sprite_count >= 8 {
                overflow_start = Some(index + 1);
                break;
            }
        }

        if let Some(start_index) = overflow_start {
            if self.sprite_overflow_bugged(start_index, target_scanline, sprite_height) {
                self.status |= STATUS_SPRITE_OVERFLOW;
            }
        }
    }

    fn sprite_row_for_scanline(
        &self,
        sprite_index: usize,
        target_scanline: u8,
        sprite_height: u8,
    ) -> Option<u8> {
        let sprite_y = self.oam[sprite_index * 4];
        let row = target_scanline.wrapping_sub(sprite_y).wrapping_sub(1);
        if row >= sprite_height {
            None
        } else {
            Some(row)
        }
    }

    fn sprite_overflow_bugged(
        &self,
        start_index: usize,
        target_scanline: u8,
        sprite_height: u8,
    ) -> bool {
        let mut n = start_index;
        let mut m = 0usize;
        while n < 64 {
            let value = self.oam[n * 4 + m];
            let row = target_scanline.wrapping_sub(value).wrapping_sub(1);
            if row < sprite_height {
                return true;
            }

            n += 1;
            m = (m + 1) & 0x03;
        }

        false
    }

    fn fetch_sprite_data(&mut self, bus: &mut impl PPUBus) {
        let slot = ((self.cycles - 257) / 8) as usize;
        if slot >= 8 {
            return;
        }

        let subcycle = (self.cycles - 257) & 0x07;
        match subcycle {
            0 | 2 => {
                let _ = self.ppu_read_bus(bus, 0x2000 | (self.loopy_v & 0x0FFF));
            }
            4 if slot < self.scanline_sprite_count as usize => {
                let sprite = self.scanline_sprites[slot];
                let (pattern_lo, _) =
                    self.fetch_sprite_pattern(bus, sprite.tile_id, sprite.attributes, sprite.row);
                self.scanline_sprites[slot].pattern_lo = pattern_lo;
            }
            6 if slot < self.scanline_sprite_count as usize => {
                let sprite = self.scanline_sprites[slot];
                let (_, pattern_hi) =
                    self.fetch_sprite_pattern(bus, sprite.tile_id, sprite.attributes, sprite.row);
                self.scanline_sprites[slot].pattern_hi = pattern_hi;
            }
            _ => {}
        }
    }

    fn draw_bg_pixel(&mut self, offset: i16, bus: &mut impl PPUBus) {
        let x = offset as usize;
        let y = self.scanline as usize;
        if x >= 256 || y >= VISIBLE_SCANLINES as usize {
            return;
        }

        let show_leftmost = (self.mask & MASK_SHOW_BG_LEFTMOST) != 0;
        let bg_pixel = if self.bg_on() && (show_leftmost || x >= 8) {
            self.current_bg_pixel()
        } else {
            0
        };
        let bg_palette = if bg_pixel == 0 {
            0
        } else {
            self.current_bg_palette()
        };

        let palette_addr = if bg_pixel == 0 {
            0x3F00
        } else {
            0x3F00 | (u16::from(bg_palette) << 2) | u16::from(bg_pixel)
        };
        let palette_data = self.ppu_read_bus(bus, palette_addr);
        let color = self.mask_palette_color(palette_data);
        let pixel_index = y * 256 + x;

        self.bit_map[pixel_index] = color;
        self.bg_colors[x] = color;
        self.bg_pixels[x] = bg_pixel;
    }

    fn draw_sprite_pixel(&mut self, offset: i16, bus: &mut impl PPUBus) {
        if !self.sprites_on() {
            return;
        }

        let x = offset as usize;
        let y = self.scanline as usize;
        if x >= 256 || y >= VISIBLE_SCANLINES as usize {
            return;
        }

        if x < 8 && (self.mask & MASK_SHOW_SPRITES_LEFTMOST) == 0 {
            return;
        }

        for sprite in self
            .scanline_sprites
            .iter()
            .take(self.scanline_sprite_count as usize)
        {
            let sprite_x = usize::from(sprite.x);
            if x < sprite_x || x >= sprite_x + 8 {
                continue;
            }

            let sprite_pixel = self.sprite_pixel(sprite, (x - sprite_x) as u8);
            if sprite_pixel == 0 {
                continue;
            }

            let bg_pixel = self.bg_pixels[x];
            if sprite.sprite_zero && bg_pixel != 0 && x < 255 {
                self.status |= STATUS_SPRITE_ZERO_HIT;
            }

            let behind_background = (sprite.attributes & 0x20) != 0;
            if behind_background && bg_pixel != 0 {
                break;
            }

            let palette = sprite.attributes & 0x03;
            let palette_addr = 0x3F10 | (u16::from(palette) << 2) | u16::from(sprite_pixel);
            let palette_data = self.ppu_read_bus(bus, palette_addr);
            let color = self.mask_palette_color(palette_data);
            self.bit_map[y * 256 + x] = color;
            break;
        }
    }

    fn set_current_vram_addr(&mut self, addr: u16) {
        self.vram_addr = addr;
        self.loopy_v = addr;
    }

    fn set_temp_vram_addr(&mut self, addr: u16) {
        self.temp_vram_addr = addr;
        self.loopy_t = addr;
    }

    fn should_skip_odd_frame_cycle(&self, pre_render_scanline: bool) -> bool {
        self.num_scanlines == 262
            && pre_render_scanline
            && self.rendering_on()
            && self.odd_frame
            && self.cycles == DOTS_PER_SCANLINE - 2
    }

    fn rendering_vram_access_active(&self) -> bool {
        self.rendering_on()
            && (self.scanline < VISIBLE_SCANLINES || self.scanline == self.num_scanlines - 1)
    }

    fn rendering_oam_access_active(&self) -> bool {
        self.rendering_on() && self.scanline < VISIBLE_SCANLINES
    }

    fn rendering_oam_clear_phase(&self) -> bool {
        self.rendering_oam_access_active() && (1..=64).contains(&self.cycles)
    }

    fn start_next_frame(&mut self) {
        self.scanline = 0;
        self.cycles = 0;
        self.frame += 1;
        self.odd_frame = !self.odd_frame;
        self.even = !self.even;
    }

    fn ppu_read_bus(&mut self, bus: &mut impl PPUBus, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        bus.check_a12(addr);
        bus.ppu_read(addr)
    }

    fn ppu_write_bus(&mut self, bus: &mut impl PPUBus, addr: u16, data: u8) {
        let addr = addr & 0x3FFF;
        bus.check_a12(addr);
        bus.ppu_write(addr, data);
    }

    fn bg_pattern_addr(&self, tile_id: u8) -> u16 {
        let table = if (self.ctrl & CTRL_BG_TABLE) != 0 {
            0x1000
        } else {
            0x0000
        };
        let fine_y = (self.loopy_v >> 12) & 0x0007;
        table | (u16::from(tile_id) << 4) | fine_y
    }

    fn fetch_sprite_pattern(
        &mut self,
        bus: &mut impl PPUBus,
        tile_id: u8,
        attributes: u8,
        row: u8,
    ) -> (u8, u8) {
        let mut fine_y = u16::from(row);
        let sprite_height = u16::from(self.sprite_height());
        if (attributes & 0x80) != 0 {
            fine_y = sprite_height - 1 - fine_y;
        }

        let addr = if sprite_height == 16 {
            let table = u16::from(tile_id & 0x01) << 12;
            let tile = u16::from(tile_id & 0xFE) + (fine_y >> 3);
            table | (tile << 4) | (fine_y & 0x07)
        } else {
            let table = if (self.ctrl & CTRL_SPRITE_TABLE) != 0 {
                0x1000
            } else {
                0x0000
            };
            table | (u16::from(tile_id) << 4) | fine_y
        };

        (
            self.ppu_read_bus(bus, addr),
            self.ppu_read_bus(bus, addr + 8),
        )
    }

    fn sprite_height(&self) -> u8 {
        if (self.ctrl & CTRL_SPRITE_SIZE) != 0 {
            16
        } else {
            8
        }
    }

    fn sprite_pixel(&self, sprite: &SpriteRenderData, offset: u8) -> u8 {
        let bit = if (sprite.attributes & 0x40) != 0 {
            offset
        } else {
            7 - offset
        };
        let lo = (sprite.pattern_lo >> bit) & 0x01;
        let hi = (sprite.pattern_hi >> bit) & 0x01;
        (hi << 1) | lo
    }

    fn mask_palette_color(&self, color: u8) -> u8 {
        let color = color & 0x3F;
        if (self.mask & MASK_GRAYSCALE) != 0 {
            color & 0x30
        } else {
            color
        }
    }

    fn update_bg_shifters(&mut self) {
        if !self.bg_on() {
            return;
        }

        self.bg_pattern_shift_lo <<= 1;
        self.bg_pattern_shift_hi <<= 1;
        self.bg_attr_shift_lo <<= 1;
        self.bg_attr_shift_hi <<= 1;
    }

    fn load_bg_shifters(&mut self) {
        self.bg_pattern_shift_lo =
            (self.bg_pattern_shift_lo & 0xFF00) | u16::from(self.next_tile_lsb);
        self.bg_pattern_shift_hi =
            (self.bg_pattern_shift_hi & 0xFF00) | u16::from(self.next_tile_msb);

        let attr_lo = if (self.next_tile_attr & 0x01) != 0 {
            0xFF
        } else {
            0x00
        };
        let attr_hi = if (self.next_tile_attr & 0x02) != 0 {
            0xFF
        } else {
            0x00
        };

        self.bg_attr_shift_lo = (self.bg_attr_shift_lo & 0xFF00) | attr_lo;
        self.bg_attr_shift_hi = (self.bg_attr_shift_hi & 0xFF00) | attr_hi;
    }

    fn current_bg_pixel(&self) -> u8 {
        let bit = 0x8000 >> self.fine_x;
        let lo = u8::from((self.bg_pattern_shift_lo & bit) != 0);
        let hi = u8::from((self.bg_pattern_shift_hi & bit) != 0);
        (hi << 1) | lo
    }

    fn current_bg_palette(&self) -> u8 {
        let bit = 0x8000 >> self.fine_x;
        let lo = u8::from((self.bg_attr_shift_lo & bit) != 0);
        let hi = u8::from((self.bg_attr_shift_hi & bit) != 0);
        (hi << 1) | lo
    }

    fn increment_x(&mut self) {
        if !self.rendering_on() {
            return;
        }

        if (self.loopy_v & 0x001F) == 31 {
            self.loopy_v &= !0x001F;
            self.loopy_v ^= 0x0400;
        } else {
            self.loopy_v = self.loopy_v.wrapping_add(1);
        }

        self.vram_addr = self.loopy_v;
    }

    fn increment_y(&mut self) {
        if !self.rendering_on() {
            return;
        }

        if (self.loopy_v & 0x7000) != 0x7000 {
            self.loopy_v = self.loopy_v.wrapping_add(0x1000);
        } else {
            self.loopy_v &= !0x7000;
            let mut coarse_y = (self.loopy_v & 0x03E0) >> 5;
            if coarse_y == 29 {
                coarse_y = 0;
                self.loopy_v ^= 0x0800;
            } else if coarse_y == 31 {
                coarse_y = 0;
            } else {
                coarse_y += 1;
            }
            self.loopy_v = (self.loopy_v & !0x03E0) | (coarse_y << 5);
        }

        self.vram_addr = self.loopy_v;
    }

    fn transfer_x(&mut self) {
        if !self.rendering_on() {
            return;
        }

        self.loopy_v = (self.loopy_v & !0x041F) | (self.loopy_t & 0x041F);
        self.vram_addr = self.loopy_v;
    }

    fn transfer_y(&mut self) {
        if !self.rendering_on() {
            return;
        }

        self.loopy_v = (self.loopy_v & !0x7BE0) | (self.loopy_t & 0x7BE0);
        self.vram_addr = self.loopy_v;
    }
}

fn palette_index_to_rgb(index: u8) -> [u8; 3] {
    NES_RGB_PALETTE[(index & 0x3F) as usize]
}

impl Default for PPU {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
