use crate::cartridge::TVSystem;

const DMC_PERIODS_NTSC: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

const DMC_PERIODS_PAL: [u16; 16] = [
    398, 354, 316, 298, 276, 236, 210, 198, 176, 148, 132, 118, 98, 78, 66, 50,
];

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum DmcDmaKind {
    Load,
    Reload,
}

#[derive(Clone, Copy)]
pub struct DmcDmaRequest {
    pub addr: u16,
    pub kind: DmcDmaKind,
}

#[derive(Clone, Copy)]
pub struct DmcState {
    pub(super) enabled: bool,
    pub(super) irq_enabled: bool,
    pub(super) loop_flag: bool,
    pub(super) irq_flag: bool,
    pub(super) output_level: u8,
    pub(super) sample_address: u16,
    pub(super) sample_length: u16,
    pub(super) current_address: u16,
    pub(super) bytes_remaining: u16,
    pub(super) sample_buffer: Option<u8>,
    pub(super) shift_register: u8,
    pub(super) bits_remaining: u8,
    pub(super) silence: bool,
    pub(super) rate_index: u8,
    pub(super) timer_reload: u16,
    pub(super) timer_counter: u16,
    pub(crate) dmc_periods: &'static [u16; 16],
}

impl Default for DmcState {
    fn default() -> Self {
        Self {
            enabled: false,
            irq_enabled: false,
            loop_flag: false,
            irq_flag: false,
            output_level: 0,
            sample_address: 0,
            sample_length: 0,
            current_address: 0,
            bytes_remaining: 0,
            sample_buffer: None,
            shift_register: 0,
            bits_remaining: 8,
            silence: true,
            rate_index: 0,
            timer_reload: 0,
            timer_counter: 0,
            dmc_periods: &DMC_PERIODS_NTSC,
        }
    }
}

impl DmcState {
    pub(super) fn set_tv_system(&mut self, tv: TVSystem) {
        self.dmc_periods = match tv {
            TVSystem::NTSC | TVSystem::DENDY => &DMC_PERIODS_NTSC,
            TVSystem::PAL => &DMC_PERIODS_PAL,
        };
        self.timer_reload = self.dmc_periods[self.rate_index as usize];
    }

    pub(super) fn write_control(&mut self, value: u8) {
        self.irq_enabled = (value & 0x80) != 0;
        self.loop_flag = (value & 0x40) != 0;
        self.rate_index = value & 0x0F;
        self.timer_reload = self.dmc_periods[self.rate_index as usize];
        if !self.irq_enabled {
            self.irq_flag = false;
        }
    }

    pub(super) fn write_output_level(&mut self, value: u8) {
        self.output_level = value & 0x7F;
    }

    pub(super) fn write_sample_address(&mut self, value: u8) {
        self.sample_address = 0xC000 | ((value as u16) << 6);
    }

    pub(super) fn write_sample_length(&mut self, value: u8) {
        self.sample_length = ((value as u16) << 4) | 0x0001;
    }

    pub(super) fn set_enabled(&mut self, enabled: bool) -> Option<DmcDmaRequest> {
        self.enabled = enabled;
        if !enabled {
            self.bytes_remaining = 0;
            self.sample_buffer = None;
            self.silence = true;
            return None;
        }

        if self.bytes_remaining == 0 {
            self.current_address = self.sample_address;
            self.bytes_remaining = self.sample_length;
            self.silence = true;
            return Some(DmcDmaRequest {
                addr: self.current_address,
                kind: DmcDmaKind::Load,
            });
        }

        None
    }

    pub(super) fn request_dma_if_needed(&self) -> Option<DmcDmaRequest> {
        if self.sample_buffer.is_none() && self.bytes_remaining > 0 {
            Some(DmcDmaRequest {
                addr: self.current_address,
                kind: DmcDmaKind::Load,
            })
        } else {
            None
        }
    }

    pub(super) fn submit_dma_sample(&mut self, value: u8) {
        self.sample_buffer = Some(value);
        self.current_address = self.current_address.wrapping_add(1);
        if self.current_address < 0x8000 {
            self.current_address = 0x8000;
        }
        if self.bytes_remaining > 0 {
            self.bytes_remaining -= 1;
            if self.bytes_remaining == 0 {
                if self.loop_flag {
                    self.current_address = self.sample_address;
                    self.bytes_remaining = self.sample_length;
                } else if self.irq_enabled {
                    self.irq_flag = true;
                }
            }
        }
    }

    pub(super) fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_reload;
            if !self.silence {
                if (self.shift_register & 1) != 0 {
                    if self.output_level <= 125 {
                        self.output_level += 2;
                    }
                } else if self.output_level >= 2 {
                    self.output_level -= 2;
                }
                self.shift_register >>= 1;
            }

            if self.bits_remaining > 0 {
                self.bits_remaining -= 1;
            }

            if self.bits_remaining == 0 {
                self.bits_remaining = 8;
                if let Some(next) = self.sample_buffer.take() {
                    self.shift_register = next;
                    self.silence = false;
                } else {
                    self.silence = true;
                }
            }
        } else {
            self.timer_counter -= 1;
        }
    }
}
