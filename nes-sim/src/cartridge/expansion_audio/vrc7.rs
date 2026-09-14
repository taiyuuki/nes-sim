use std::cell::RefCell;
use std::rc::Rc;

use crate::apu::ExpansionAudioChip;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// The OPLL on the VRC7 runs at master-clock / 48, i.e. one operator slot per
// CPU cycle across a 36-slot rotation of the six channels.
const SLOT_ROTATION: u64 = 36;
const CHANNELS: usize = 6;

// Envelope unit constants (23-bit attenuation domain).
const ZEROVOL: f64 = 8_388_608.0;
const MAXVOL: f64 = 0.0;
const CUTOFF_STEP: f64 = 16_384.0;

// LFO periods expressed in slot clocks: the reference tables sample a 6.4 Hz
// vibrato and 3.7 Hz tremolo at the slot rate of CPU/6.
const VIBRATO_DEPTH: f64 = 10.0;
const AM_DEPTH: f64 = 128.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EnvState {
    Cutoff,
    Attack,
    Decay,
    Release,
}

impl EnvState {
    fn encode(self) -> u8 {
        match self {
            EnvState::Cutoff => 0,
            EnvState::Attack => 1,
            EnvState::Decay => 2,
            EnvState::Release => 3,
        }
    }

    fn decode(value: u8) -> Self {
        match value {
            1 => EnvState::Attack,
            2 => EnvState::Decay,
            3 => EnvState::Release,
            _ => EnvState::Cutoff,
        }
    }
}

const MULTIPLIER: [f64; 16] = [
    0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 10.0, 12.0, 12.0, 15.0, 15.0,
];

const KEYSCALE: [f64; 8] = [0.0, 1536.0, 2048.0, 2368.0, 2560.0, 2752.0, 2880.0, 3008.0];

const ATTACK_VAL: [f64; 76] = [
    0.0,
    0.0,
    0.0,
    0.0,
    98.0,
    120.0,
    146.0,
    171.0,
    195.0,
    216.0,
    293.0,
    341.0,
    390.0,
    471.0,
    602.0,
    683.0,
    780.0,
    964.0,
    1168.0,
    1366.0,
    1560.0,
    1927.0,
    2315.0,
    2731.0,
    3075.0,
    3855.0,
    4682.0,
    5461.0,
    6242.0,
    8035.0,
    9364.0,
    10921.0,
    12480.0,
    15423.0,
    18727.0,
    21856.0,
    24960.0,
    30847.0,
    37413.0,
    43713.0,
    51130.0,
    61580.0,
    74991.0,
    87425.0,
    99841.0,
    123161.0,
    149319.0,
    173949.0,
    200870.0,
    241044.0,
    281218.0,
    312464.0,
    337461.0,
    401739.0,
    496266.0,
    562435.0,
    602609.0,
    766957.0,
    937392.0,
    1205218.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
    8_388_607.0,
];

const DECAY_VAL: [f64; 76] = [
    0.0, 0.0, 0.0, 0.0, 8.0, 10.0, 12.0, 14.0, 16.0, 20.0, 24.0, 28.0, 32.0, 40.0, 48.0, 56.0,
    65.0, 77.0, 96.0, 112.0, 129.0, 161.0, 193.0, 224.0, 258.0, 321.0, 386.0, 449.0, 516.0, 643.0,
    771.0, 898.0, 1032.0, 1285.0, 1542.0, 1796.0, 2064.0, 2570.0, 3084.0, 3591.0, 4211.0, 5268.0,
    6167.0, 7183.0, 8255.0, 10282.0, 12407.0, 14360.0, 16510.0, 20552.0, 24668.0, 28745.0, 33020.0,
    41154.0, 49336.0, 57391.0, 66169.0, 82308.0, 98673.0, 114783.0, 132859.0, 132859.0, 132859.0,
    132859.0, 132859.0, 132859.0, 132859.0, 132859.0, 132859.0, 132859.0, 132859.0, 132859.0,
    132859.0, 132859.0, 132859.0, 132859.0,
];

const INSTRUMENTS: [[u8; 8]; 16] = [
    [0x00; 8],                                        // Custom patch, provided through $00-$07
    [0x03, 0x21, 0x05, 0x06, 0xE8, 0x81, 0x42, 0x27], // Bell
    [0x13, 0x41, 0x14, 0x0D, 0xD8, 0xF6, 0x23, 0x12], // Guitar
    [0x11, 0x11, 0x08, 0x08, 0xFA, 0xB2, 0x20, 0x12], // Wurlitzer
    [0x31, 0x61, 0x0C, 0x07, 0xA8, 0x64, 0x61, 0x27], // Flute
    [0x32, 0x21, 0x1E, 0x06, 0xE1, 0x76, 0x01, 0x28], // Clarinet
    [0x02, 0x01, 0x06, 0x00, 0xA3, 0xE2, 0xF4, 0xF4], // Synth
    [0x21, 0x61, 0x1D, 0x07, 0x82, 0x81, 0x11, 0x07], // Trumpet
    [0x23, 0x21, 0x22, 0x17, 0xA2, 0x72, 0x01, 0x17], // Organ
    [0x35, 0x11, 0x25, 0x00, 0x40, 0x73, 0x72, 0x01], // Bells
    [0xB5, 0x01, 0x0F, 0x0F, 0xA8, 0xA5, 0x51, 0x02], // Vibes
    [0x17, 0xC1, 0x24, 0x07, 0xF8, 0xF8, 0x22, 0x12], // Vibraphone
    [0x71, 0x23, 0x11, 0x06, 0x65, 0x74, 0x18, 0x16], // Tutti
    [0x01, 0x02, 0xD3, 0x05, 0xC9, 0x95, 0x03, 0x02], // Fretless
    [0x61, 0x63, 0x0C, 0x00, 0x94, 0xC0, 0x33, 0xF6], // Synth Bass
    [0x21, 0x72, 0x0D, 0x00, 0xC1, 0xD5, 0x56, 0x06], // Sweep
];

fn tri(x: f64) -> f64 {
    let x = x % (2.0 * std::f64::consts::PI);
    if x < std::f64::consts::FRAC_PI_2 {
        x / std::f64::consts::PI
    } else if x < 3.0 * std::f64::consts::FRAC_PI_2 {
        1.0 - x / std::f64::consts::PI
    } else {
        x / std::f64::consts::PI - 2.0
    }
}

struct Tables {
    log_sin: [f64; 256],
    exp: [f64; 256],
}

impl Tables {
    fn new() -> Self {
        let mut log_sin = [0.0; 256];
        for (i, slot) in log_sin.iter_mut().enumerate() {
            *slot = (-(((i as f64) + 0.5) * std::f64::consts::PI / 256.0 / 2.0)
                .sin()
                .ln()
                / std::f64::consts::LN_2
                * 256.0)
                .round();
        }

        let mut exp = [0.0; 256];
        for (i, slot) in exp.iter_mut().enumerate() {
            *slot = (((2.0f64).powf(i as f64 / 256.0) - 1.0) * 1024.0).round();
        }

        Self { log_sin, exp }
    }
}

/// YM2413 (OPLL) tone generator as used on the VRC7. The six melodic
/// channels are modeled; rhythm-mode channels share the same machinery.
pub(crate) struct Vrc7Audio {
    tables: Box<Tables>,
    mod_env_state: [EnvState; CHANNELS],
    car_env_state: [EnvState; CHANNELS],
    vol: [u8; CHANNELS],
    freq: [u16; CHANNELS],
    octave: [u8; CHANNELS],
    instrument: [u8; CHANNELS],
    mod_out: [f64; CHANNELS],
    old_mod_out: [f64; CHANNELS],
    out: [f64; CHANNELS],
    key_on: [bool; CHANNELS],
    channel_sustain: [bool; CHANNELS],
    lfo_ctr: u64,
    am_ctr: u64,
    phase: [f64; CHANNELS],
    user_tone: [u8; 8],
    mod_env_vol: [f64; CHANNELS],
    car_env_vol: [f64; CHANNELS],
    slot: u64,
    lp_accum: i32,
    lp_accum2: i32,
    sign: f64,
}

impl Vrc7Audio {
    pub(crate) fn new() -> Self {
        Self {
            tables: Box::new(Tables::new()),
            mod_env_state: [EnvState::Cutoff; CHANNELS],
            car_env_state: [EnvState::Cutoff; CHANNELS],
            vol: [0; CHANNELS],
            freq: [0; CHANNELS],
            octave: [0; CHANNELS],
            instrument: [0; CHANNELS],
            mod_out: [0.0; CHANNELS],
            old_mod_out: [0.0; CHANNELS],
            out: [0.0; CHANNELS],
            key_on: [false; CHANNELS],
            channel_sustain: [false; CHANNELS],
            lfo_ctr: 0,
            am_ctr: 0,
            phase: [0.0; CHANNELS],
            user_tone: [0; 8],
            mod_env_vol: [511.0; CHANNELS],
            car_env_vol: [511.0; CHANNELS],
            slot: 0,
            lp_accum: 0,
            lp_accum2: 0,
            sign: 1.0,
        }
    }

    pub(crate) fn write(&mut self, register: u8, data: u8) {
        match register {
            0..=7 => self.user_tone[(register & 7) as usize] = data,
            0x10..=0x15 => {
                let ch = (register - 0x10) as usize;
                self.freq[ch] = (self.freq[ch] & 0x0F00) | u16::from(data);
            }
            0x20..=0x25 => {
                let ch = (register - 0x20) as usize;
                self.octave[ch] = (data >> 1) & 0x07;
                self.freq[ch] = (self.freq[ch] & 0x00FF) | (u16::from(data & 0x01) << 8);
                if (data & 0x10) != 0 && !self.key_on[ch] {
                    self.car_env_state[ch] = EnvState::Cutoff;
                    self.mod_env_state[ch] = EnvState::Cutoff;
                }
                self.key_on[ch] = (data & 0x10) != 0;
                self.channel_sustain[ch] = (data & 0x20) != 0;
            }
            0x30..=0x35 => {
                let ch = (register - 0x30) as usize;
                self.vol[ch] = data & 0x0F;
                self.instrument[ch] = (data >> 4) & 0x0F;
            }
            _ => {}
        }
    }

    fn instrument_data(&self, ch: usize) -> [u8; 8] {
        if self.instrument[ch] == 0 {
            self.user_tone
        } else {
            INSTRUMENTS[self.instrument[ch] as usize]
        }
    }

    pub(crate) fn tick(&mut self) {
        self.slot = (self.slot + 1) % SLOT_ROTATION;
        if self.slot < CHANNELS as u64 {
            self.operate(self.slot as usize);
        }
    }

    fn operate(&mut self, ch: usize) {
        self.lfo_ctr = self.lfo_ctr.wrapping_add(1);
        self.am_ctr = self.am_ctr.wrapping_add(1);
        self.phase[ch] += ((self.freq[ch] as u32) << self.octave[ch]) as f64 / 512.0;
        self.phase[ch] %= 1024.0;

        let inst = self.instrument_data(ch);

        let mod_envelope = self.set_envelope(ch, false, &inst) * 4.0;
        let car_envelope = self.set_envelope(ch, true, &inst) * 4.0;

        let keyscale = (KEYSCALE[(self.freq[ch] >> 5) as usize & 0x07]
            - 512.0 * (7 - self.octave[ch]) as f64)
            .max(0.0);
        let mod_ks_level = (inst[2] >> 6) as u32;
        let mod_ks = if mod_ks_level == 0 {
            0.0
        } else {
            keyscale / f64::from(1u32 << (3 - mod_ks_level))
        };
        let car_ks_level = (inst[3] >> 6) as u32;
        let car_ks = if car_ks_level == 0 {
            0.0
        } else {
            keyscale / f64::from(1u32 << (3 - car_ks_level))
        };

        let feedback = (!inst[3]) & 7;
        let mod_vibrato = if (inst[0] & 0x40) == 0 {
            0.0
        } else {
            VIBRATO_DEPTH
                * tri(2.0 * std::f64::consts::PI * 6.4 * self.lfo_ctr as f64 / (1_789_773.0 / 6.0))
                * f64::from(1u32 << self.octave[ch])
        };
        let mod_multiplier = MULTIPLIER[(inst[0] & 0x0F) as usize];
        let mod_feedback = if feedback == 7 {
            0.0
        } else {
            ((self.mod_out[ch] as i64 + self.old_mod_out[ch] as i64) >> (2 + feedback)) as f64
        };
        let mod_f = mod_feedback + (mod_vibrato + mod_multiplier * self.phase[ch]);
        let mod_vol = f64::from(inst[2] & 0x3F) * 32.0;
        let mod_am = if (inst[0] & 0x80) == 0 {
            0.0
        } else {
            (AM_DEPTH
                * tri(2.0 * std::f64::consts::PI * 3.7 * self.am_ctr as f64 / (1_789_773.0 / 6.0))
                + AM_DEPTH)
                .floor()
        };
        let mod_rectify = (inst[3] & 0x08) != 0;

        self.mod_out[ch] =
            self.operator(mod_f, mod_vol + mod_envelope + mod_ks + mod_am, mod_rectify) * 4.0;
        self.old_mod_out[ch] = self.mod_out[ch];

        let car_vibrato = if (inst[1] & 0x40) == 0 {
            0.0
        } else {
            VIBRATO_DEPTH
                * tri(2.0 * std::f64::consts::PI * 6.4 * self.lfo_ctr as f64 / (1_789_773.0 / 6.0))
                * (((self.freq[ch] as u32) << self.octave[ch]) as f64 / 512.0)
        };
        let car_multiplier = MULTIPLIER[(inst[1] & 0x0F) as usize];
        let car_feedback = ((self.mod_out[ch] as i64 + self.old_mod_out[ch] as i64) >> 1) as f64;
        let car_f = car_feedback + (car_vibrato + car_multiplier * self.phase[ch]);
        let car_vol = f64::from(self.vol[ch]) * 128.0;
        let car_am = if (inst[1] & 0x80) == 0 {
            0.0
        } else {
            (AM_DEPTH
                * tri(2.0 * std::f64::consts::PI * 3.7 * self.am_ctr as f64 / (1_789_773.0 / 6.0))
                + AM_DEPTH)
                .floor()
        };
        let car_rectify = (inst[3] & 0x10) != 0;

        self.out[ch] =
            self.operator(car_f, car_vol + car_envelope + car_ks + car_am, car_rectify) * 4.0;
        self.mix_channel(ch);
    }

    fn operator(&mut self, phase: f64, gain: f64, rectify: bool) -> f64 {
        let log_sin = self.log_sin(phase, rectify);
        self.exp(log_sin + gain)
    }

    fn exp(&mut self, val: f64) -> f64 {
        let val = val.clamp(0.0, 8190.0);
        let n = (-val) as i32;
        let mantissa = self.tables.exp[(n & 0xFF) as usize];
        let exponent = n >> 8;
        let shifted = ((mantissa + 1024.0) as i64 >> (-exponent)) as f64;
        shifted * self.sign
    }

    fn log_sin(&mut self, x: f64, rectify: bool) -> f64 {
        let xi = x as i32;
        let index = (xi & 0xFF) as usize;
        match (xi >> 8) & 3 {
            0 => {
                self.sign = 1.0;
                self.tables.log_sin[index]
            }
            1 => {
                self.sign = 1.0;
                self.tables.log_sin[255 - index]
            }
            2 => {
                self.sign = if rectify { 0.0 } else { -1.0 };
                self.tables.log_sin[index]
            }
            _ => {
                self.sign = if rectify { 0.0 } else { -1.0 };
                self.tables.log_sin[255 - index]
            }
        }
    }

    fn mix_channel(&mut self, ch: usize) {
        let sample = (self.out[ch] * 24.0) as i64 + self.lp_accum as i64;
        self.lp_accum -= (sample >> 2) as i32;
        let j = self.lp_accum as i64 + self.lp_accum2 as i64;
        self.lp_accum2 -= (j >> 2) as i32;
    }

    pub(crate) fn output(&self) -> f32 {
        // Roughly match the NES APU channel headroom: the internal low-pass
        // accumulator stays within a couple hundred thousand counts.
        (self.lp_accum2 as f32 / 200_000.0 * 0.4).clamp(-0.6, 0.6)
    }

    fn set_envelope(&mut self, ch: usize, is_carrier: bool, inst: &[u8; 8]) -> f64 {
        let keyscale_rate = (inst[if is_carrier { 1 } else { 0 }] & 0x10) != 0;
        // Key code is (block << 1) | fnum bit 8. When key-scale-rate is on,
        // its top two bits speed the rate index up and the low two select a
        // fine step, mirroring the hardware rate generator.
        let keycode = ((self.octave[ch] as u16) << 1) | (self.freq[ch] >> 8);
        let (rate_add, fine_step) = if keyscale_rate {
            (((keycode >> 2) & 0x03) as usize, (keycode & 0x03) as usize)
        } else {
            (0, (self.octave[ch] >> 1) as usize)
        };
        let attack_rate = ((inst[if is_carrier { 5 } else { 4 }] >> 4) & 0x0F) as usize;
        let decay_rate = (inst[if is_carrier { 5 } else { 4 }] & 0x0F) as usize;
        let sustain_level = ((inst[if is_carrier { 7 } else { 6 }] >> 4) & 0x0F) as usize;
        let release_rate = (inst[if is_carrier { 7 } else { 6 }] & 0x0F) as usize;
        let rate_index = |rate: usize| (rate + rate_add) * 4 + fine_step;
        let state = if is_carrier {
            self.car_env_state[ch]
        } else {
            self.mod_env_state[ch]
        };
        let mut vol = if is_carrier {
            self.car_env_vol[ch]
        } else {
            self.mod_env_vol[ch]
        };
        match state {
            EnvState::Attack => {
                if vol > MAXVOL + 8.0 {
                    vol -= ATTACK_VAL[rate_index(attack_rate)];
                } else {
                    self.assign_env_state(ch, is_carrier, EnvState::Decay);
                }
                if !self.key_on[ch] {
                    self.assign_env_state(ch, is_carrier, EnvState::Release);
                }
            }
            EnvState::Decay => {
                if vol < sustain_level as f64 * 524_288.0 {
                    vol += DECAY_VAL[rate_index(decay_rate)];
                } else {
                    self.assign_env_state(ch, is_carrier, EnvState::Release);
                }
                if !self.key_on[ch] {
                    self.assign_env_state(ch, is_carrier, EnvState::Release);
                }
            }
            EnvState::Release => {
                // Bit 5 of the operator register selects whether the
                // release or the sustain rate applies while the key is held.
                let d5 = (inst[if is_carrier { 1 } else { 0 }] & 0x20) != 0;
                let sustain = self.channel_sustain[ch];
                if self.key_on[ch] {
                    if !d5 {
                        vol += DECAY_VAL[rate_index(release_rate)];
                    }
                } else if d5 {
                    if sustain {
                        vol += DECAY_VAL[rate_index(5)];
                    } else {
                        vol += DECAY_VAL[rate_index(release_rate)];
                    }
                } else if sustain {
                    vol += DECAY_VAL[rate_index(5)];
                } else {
                    vol += DECAY_VAL[rate_index(7)];
                }
            }
            EnvState::Cutoff => {
                if vol < ZEROVOL {
                    vol += CUTOFF_STEP;
                } else {
                    vol = ZEROVOL;
                    if self.key_on[ch] {
                        self.assign_env_state(ch, is_carrier, EnvState::Attack);
                        self.phase[ch] = 0.0;
                    }
                }
            }
        }

        vol = vol.clamp(MAXVOL, ZEROVOL);
        if is_carrier {
            self.car_env_vol[ch] = vol;
        } else {
            self.mod_env_vol[ch] = vol;
        }

        let state = if is_carrier {
            self.car_env_state[ch]
        } else {
            self.mod_env_state[ch]
        };
        if state == EnvState::Attack {
            // Exponential attack compensation; a fresh key-on at maximum
            // attenuation yields zero instead of infinity.
            let headroom = ZEROVOL - vol;
            if headroom <= 0.0 {
                return 0.0;
            }
            let output = ZEROVOL - (ZEROVOL * headroom.ln() / ZEROVOL.ln()).floor();
            return output / 16_384.0;
        }
        vol / 16_384.0
    }

    fn assign_env_state(&mut self, ch: usize, is_carrier: bool, state: EnvState) {
        if is_carrier {
            self.car_env_state[ch] = state;
        } else {
            self.mod_env_state[ch] = state;
        }
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        for ch in 0..CHANNELS {
            writer.write_u8(self.mod_env_state[ch].encode());
            writer.write_u8(self.car_env_state[ch].encode());
            writer.write_u8(self.vol[ch]);
            writer.write_u16(self.freq[ch]);
            writer.write_u8(self.octave[ch]);
            writer.write_u8(self.instrument[ch]);
            writer.write_bool(self.key_on[ch]);
            writer.write_bool(self.channel_sustain[ch]);
            writer.write_u64(self.mod_out[ch].to_bits());
            writer.write_u64(self.out[ch].to_bits());
            writer.write_u64(self.phase[ch].to_bits());
            writer.write_u64(self.mod_env_vol[ch].to_bits());
            writer.write_u64(self.car_env_vol[ch].to_bits());
        }
        writer.write_bytes(&self.user_tone);
        writer.write_u64(self.lfo_ctr);
        writer.write_u64(self.am_ctr);
        writer.write_u64(self.slot);
        writer.write_u32(self.lp_accum as u32);
        writer.write_u32(self.lp_accum2 as u32);
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        for ch in 0..CHANNELS {
            self.mod_env_state[ch] = EnvState::decode(reader.read_u8()?);
            self.car_env_state[ch] = EnvState::decode(reader.read_u8()?);
            self.vol[ch] = reader.read_u8()?;
            self.freq[ch] = reader.read_u16()?;
            self.octave[ch] = reader.read_u8()?;
            self.instrument[ch] = reader.read_u8()?;
            self.key_on[ch] = reader.read_bool()?;
            self.channel_sustain[ch] = reader.read_bool()?;
            self.mod_out[ch] = f64::from_bits(reader.read_u64()?);
            self.out[ch] = f64::from_bits(reader.read_u64()?);
            self.phase[ch] = f64::from_bits(reader.read_u64()?);
            self.mod_env_vol[ch] = f64::from_bits(reader.read_u64()?);
            self.car_env_vol[ch] = f64::from_bits(reader.read_u64()?);
        }
        reader.read_bytes_into(&mut self.user_tone)?;
        self.lfo_ctr = reader.read_u64()?;
        self.am_ctr = reader.read_u64()?;
        self.slot = reader.read_u64()?;
        self.lp_accum = reader.read_u32()? as i32;
        self.lp_accum2 = reader.read_u32()? as i32;
        Ok(())
    }
}

pub(crate) struct Vrc7AudioChip {
    audio: Rc<RefCell<Vrc7Audio>>,
}

impl Vrc7AudioChip {
    pub(crate) fn new(audio: Rc<RefCell<Vrc7Audio>>) -> Self {
        Self { audio }
    }
}

impl ExpansionAudioChip for Vrc7AudioChip {
    fn tick_cpu_cycle(&mut self) {
        self.audio.borrow_mut().tick();
    }

    fn output_sample(&self) -> f32 {
        self.audio.borrow().output()
    }
}
