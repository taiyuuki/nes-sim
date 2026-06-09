use std::cell::RefCell;
use std::rc::Rc;

use crate::apu::ExpansionAudioChip;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const RAM_SIZE: usize = 0x80;
const CHANNEL_COUNT: usize = 8;
const CYCLE_DIVIDER: u8 = 15;

struct Channel {
    phase: u32,
    output: i32,
}

impl Channel {
    fn new() -> Self {
        Self {
            phase: 0,
            output: 0,
        }
    }
}

pub(crate) struct Namco163Audio {
    ram: [u8; RAM_SIZE],
    addr_reg: u8,
    channels: [Channel; CHANNEL_COUNT],
    num_channels: u8,
    cycpos: u8,
    curch: u8,
    lpaccum: i32,
}

impl Namco163Audio {
    pub(crate) fn new() -> Self {
        let mut ram = [0u8; RAM_SIZE];
        ram[0x7F] = 0x10;
        Self {
            ram,
            addr_reg: 0,
            channels: [
                Channel::new(),
                Channel::new(),
                Channel::new(),
                Channel::new(),
                Channel::new(),
                Channel::new(),
                Channel::new(),
                Channel::new(),
            ],
            num_channels: 2,
            cycpos: 0,
            curch: 0,
            lpaccum: 0,
        }
    }

    pub(crate) fn read_ram(&mut self) -> u8 {
        let data = self.ram[(self.addr_reg & 0x7F) as usize];
        if self.addr_reg & 0x80 != 0 {
            self.addr_reg = (self.addr_reg & 0x80) | ((self.addr_reg + 1) & 0x7F);
        }
        data
    }

    pub(crate) fn write_addr(&mut self, data: u8) {
        self.addr_reg = data;
    }

    pub(crate) fn write_ram(&mut self, data: u8) {
        let addr = (self.addr_reg & 0x7F) as usize;
        self.ram[addr] = data;

        if addr >= 0x40 {
            let ch = (addr - 0x40) >> 3;
            self.update_channel(ch);
        }

        if self.addr_reg & 0x80 != 0 {
            self.addr_reg = (self.addr_reg & 0x80) | ((self.addr_reg + 1) & 0x7F);
        }
    }

    fn read_ram_at(&self, addr: usize) -> u8 {
        self.ram[addr & 0x7F]
    }

    fn channel_frequency(&self, ch: usize) -> u32 {
        let off = 0x40 + ch * 8;
        (self.read_ram_at(off) as u32)
            | ((self.read_ram_at(off + 2) as u32) << 8)
            | ((self.read_ram_at(off + 4) as u32 & 0x03) << 16)
    }

    fn channel_wave_len(&self, ch: usize) -> u32 {
        let off = 0x40 + ch * 8;
        let reg4 = self.read_ram_at(off + 4);
        (0x100 - (reg4 & 0xFC) as u32) << 16
    }

    fn channel_wave_start(&self, ch: usize) -> usize {
        let off = 0x40 + ch * 8;
        self.read_ram_at(off + 6) as usize
    }

    fn channel_volume(&self, ch: usize) -> u8 {
        let off = 0x40 + ch * 8;
        self.read_ram_at(off + 7) & 0x0F
    }

    fn channel_enabled(&self, ch: usize) -> bool {
        let off = 0x40 + ch * 8;
        let reg4 = self.read_ram_at(off + 4);
        reg4 & 0xE0 != 0
    }

    fn update_channel(&mut self, ch: usize) {
        self.num_channels = 1 + ((self.read_ram_at(0x7F) >> 4) & 0x07);
        if !self.channel_enabled(ch) || self.channel_volume(ch) == 0 {
            self.channels[ch].output = 0;
        }
    }

    fn get_wave_sample(&self, addr: usize) -> i32 {
        let b = self.read_ram_at(addr >> 1);
        let nibble = if (addr & 1) == 0 { b & 0x0F } else { b >> 4 };
        (nibble as i32) - 8
    }

    fn clock_channel(&mut self, ch: usize) {
        if !self.channel_enabled(ch) {
            self.channels[ch].output = 0;
            return;
        }
        let vol = self.channel_volume(ch);
        if vol == 0 {
            self.channels[ch].output = 0;
            return;
        }
        let freq = self.channel_frequency(ch);
        let wave_len = self.channel_wave_len(ch);
        let wave_start = self.channel_wave_start(ch);

        let phase = self.channels[ch].phase;
        let new_phase = (phase + freq) % wave_len;
        self.channels[ch].phase = new_phase;

        let sample_addr = (wave_start + (new_phase >> 16) as usize) & 0xFF;
        let wave_sample = self.get_wave_sample(sample_addr);
        self.channels[ch].output = wave_sample * (vol as i32) * 16;
    }

    pub(crate) fn tick(&mut self) {
        self.num_channels = 1 + ((self.read_ram_at(0x7F) >> 4) & 0x07);
        self.cycpos = (self.cycpos + 1) % CYCLE_DIVIDER;
        if self.cycpos == 0 {
            self.curch = (self.curch + 1) % self.num_channels;
            self.clock_channel(self.curch as usize);
        }
    }

    pub(crate) fn output(&self) -> f32 {
        let mut sample: i32 = 0;
        for i in 0..self.num_channels as usize {
            sample += self.channels[i].output;
        }
        sample += self.lpaccum;
        (sample as f32) / 4096.0
    }

    pub(crate) fn apply_lowpass(&mut self) {
        let mut sample: i32 = 0;
        for i in 0..self.num_channels as usize {
            sample += self.channels[i].output;
        }
        sample += self.lpaccum;
        self.lpaccum = sample - (sample >> 4);
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.ram);
        writer.write_u8(self.addr_reg);
        writer.write_u8(self.num_channels);
        writer.write_u8(self.cycpos);
        writer.write_u8(self.curch);
        writer.write_u32(self.lpaccum as u32);
        for ch in &self.channels {
            writer.write_u32(ch.phase);
            writer.write_u32(ch.output as u32);
        }
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.ram)?;
        self.addr_reg = reader.read_u8()?;
        self.num_channels = reader.read_u8()?;
        self.cycpos = reader.read_u8()?;
        self.curch = reader.read_u8()?;
        self.lpaccum = reader.read_u32()? as i32;
        for ch in &mut self.channels {
            ch.phase = reader.read_u32()?;
            ch.output = reader.read_u32()? as i32;
        }
        Ok(())
    }
}

pub(crate) struct Namco163AudioChip {
    audio: Rc<RefCell<Namco163Audio>>,
}

impl Namco163AudioChip {
    pub(crate) fn new(audio: Rc<RefCell<Namco163Audio>>) -> Self {
        Self { audio }
    }
}

impl ExpansionAudioChip for Namco163AudioChip {
    fn tick_cpu_cycle(&mut self) {
        let mut audio = self.audio.borrow_mut();
        audio.tick();
        audio.apply_lowpass();
    }

    fn output_sample(&self) -> f32 {
        self.audio.borrow().output()
    }
}
