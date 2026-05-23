use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const IRQ_PRESCALER_PERIOD: i32 = 341;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Vrc4 {
    prg_rom: Vec<u8>,
    chr_memory: ChrMemory,
    prg_bank_0: u8,
    prg_bank_1: u8,
    chr_banks: [u8; 8],
    mirroring: Mirroring,
    prg_mode: bool,
    irq_latch: u8,
    irq_counter: u8,
    irq_prescaler: i32,
    irq_enabled: bool,
    irq_mode: bool,
    irq_pending: bool,
    irq_ack: bool,
    /// (bit0_a, bit1_a, bit0_b, bit1_b) — 从地址线中提取 bit0/bit1 的位移
    reg_bits: (u8, u8, u8, u8),
}

impl Vrc4 {
    pub(super) fn new(
        prg_rom: Vec<u8>,
        chr_data: Vec<u8>,
        mirroring: Mirroring,
        mapper_id: u16,
    ) -> Self {
        let chr_memory = if chr_data.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_data)
        };

        let reg_bits = match mapper_id {
            21 => (1, 2, 6, 7), // VRC4a: bit0=A1|A6, bit1=A2|A7
            23 => (2, 3, 0, 1), // VRC4e: bit0=A2|A0, bit1=A3|A1
            25 => (3, 2, 1, 0), // VRC4b: bit0=A3|A1, bit1=A2|A0
            _ => (1, 2, 6, 7),
        };

        Self {
            prg_rom,
            chr_memory,
            prg_bank_0: 0,
            prg_bank_1: 1,
            chr_banks: [0, 1, 2, 3, 4, 5, 6, 7],
            mirroring,
            prg_mode: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_prescaler: IRQ_PRESCALER_PERIOD,
            irq_enabled: false,
            irq_mode: false,
            irq_pending: false,
            irq_ack: false,
            reg_bits,
        }
    }

    fn extract_bits(&self, addr: u16) -> (bool, bool) {
        let (a0, a1, b0, b1) = self.reg_bits;
        let bit0 = (addr & (1 << a0)) != 0 || (addr & (1 << b0)) != 0;
        let bit1 = (addr & (1 << a1)) != 0 || (addr & (1 << b1)) != 0;
        (bit0, bit1)
    }

    fn tick_irq(&mut self) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_mode {
            self.scanline_count();
        } else {
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += IRQ_PRESCALER_PERIOD;
                self.scanline_count();
            }
        }
    }

    fn scanline_count(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        }
    }
}

impl Mapper for Vrc4 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let last_8k = self.prg_rom.len().saturating_sub(PRG_BANK_8K);
                let second_last_8k = last_8k.saturating_sub(PRG_BANK_8K);

                let offset = if self.prg_mode {
                    match addr {
                        0x8000..=0x9FFF => {
                            (addr - 0x8000) as usize + last_8k
                        }
                        0xA000..=0xBFFF => {
                            let bank = self.prg_bank_0 as usize;
                            (addr - 0xA000) as usize + bank * PRG_BANK_8K
                        }
                        0xC000..=0xDFFF => {
                            let bank = self.prg_bank_1 as usize;
                            (addr - 0xC000) as usize + bank * PRG_BANK_8K
                        }
                        _ => {
                            // $E000-$FFFF: fixed last 8K
                            (addr - 0xE000) as usize + last_8k
                        }
                    }
                } else {
                    match addr {
                        0x8000..=0x9FFF => {
                            let bank = self.prg_bank_0 as usize;
                            (addr - 0x8000) as usize + bank * PRG_BANK_8K
                        }
                        0xA000..=0xBFFF => {
                            let bank = self.prg_bank_1 as usize;
                            (addr - 0xA000) as usize + bank * PRG_BANK_8K
                        }
                        _ => {
                            // $C000-$FFFF: fixed last 16K
                            (addr - 0xC000) as usize + second_last_8k
                        }
                    }
                };
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                let (bit0, bit1) = self.extract_bits(addr);
                match addr & 0xF000 {
                    0x8000 => {
                        self.prg_bank_0 = data & 0x1F;
                    }
                    0x9000 => {
                        if bit1 {
                            self.prg_mode = (data & 0x02) != 0;
                        } else {
                            self.mirroring = match data & 0x03 {
                                0 => Mirroring::Vertical,
                                1 => Mirroring::Horizontal,
                                2 => Mirroring::SPAGE0,
                                3 => Mirroring::SPAGE1,
                                _ => unreachable!(),
                            };
                        }
                    }
                    0xA000 => {
                        self.prg_bank_1 = data & 0x1F;
                    }
                    0xB000 | 0xC000 | 0xD000 | 0xE000 => {
                        let data = data & 0x0F;
                        let which_reg = ((addr as usize - 0xB000) >> 11) as u8
                            + if bit1 { 1 } else { 0 };
                        let old_val = self.chr_banks[which_reg as usize];
                        if bit0 {
                            self.chr_banks[which_reg as usize] =
                                (old_val & 0x0F) | (data << 4);
                        } else {
                            self.chr_banks[which_reg as usize] =
                                (old_val & 0xF0) | data;
                        }
                    }
                    0xF000 => {
                        match (bit1, bit0) {
                            (false, false) => {
                                // F000: IRQ latch 低 4 位
                                self.irq_latch =
                                    (self.irq_latch & 0xF0) | (data & 0x0F);
                            }
                            (false, true) => {
                                // F001: IRQ latch 高 4 位
                                self.irq_latch =
                                    (self.irq_latch & 0x0F) | ((data & 0x0F) << 4);
                            }
                            (true, false) => {
                                // F002: IRQ 控制
                                self.irq_ack = (data & 0x01) != 0;
                                self.irq_enabled = (data & 0x02) != 0;
                                self.irq_mode = (data & 0x04) != 0;
                                if self.irq_enabled {
                                    self.irq_counter = self.irq_latch;
                                    self.irq_prescaler = IRQ_PRESCALER_PERIOD;
                                }
                                self.irq_pending = false;
                            }
                            (true, true) => {
                                // F003: IRQ toggle
                                self.irq_enabled = self.irq_ack;
                                self.irq_pending = false;
                            }
                        }
                    }
                    _ => {}
                }
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        let addr = addr as usize & 0x1FFF;
        match &self.chr_memory {
            ChrMemory::Rom(rom) => {
                let bank = self.chr_banks[addr / CHR_BANK_1K] as usize;
                let offset = (addr % CHR_BANK_1K) + bank * CHR_BANK_1K;
                Some(rom[offset % rom.len()])
            }
            ChrMemory::Ram(ram) => Some(ram[addr]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if let ChrMemory::Ram(ram) = &mut self.chr_memory {
            ram[addr as usize & 0x1FFF] = data;
            return true;
        }
        false
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_line(&self) -> bool {
        self.irq_pending
    }

    fn tick_cpu_cycle(&mut self) {
        self.tick_irq();
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.prg_bank_0);
        writer.write_u8(self.prg_bank_1);
        for &bank in &self.chr_banks {
            writer.write_u8(bank);
        }
        writer.write_u8(self.mirroring as u8);
        writer.write_bool(self.prg_mode);
        writer.write_u8(self.irq_latch);
        writer.write_u8(self.irq_counter);
        writer.write_i16(self.irq_prescaler as i16);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_mode);
        writer.write_bool(self.irq_pending);
        writer.write_bool(self.irq_ack);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank_0 = reader.read_u8()?;
        self.prg_bank_1 = reader.read_u8()?;
        for bank in &mut self.chr_banks {
            *bank = reader.read_u8()?;
        }
        self.mirroring = match reader.read_u8()? {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::FourScreen,
            3 => Mirroring::SPAGE0,
            4 => Mirroring::SPAGE1,
            _ => {
                return Err(SaveStateError::InvalidData(
                    "invalid mirroring value in VRC4 state",
                ))
            }
        };
        self.prg_mode = reader.read_bool()?;
        self.irq_latch = reader.read_u8()?;
        self.irq_counter = reader.read_u8()?;
        self.irq_prescaler = reader.read_i16()? as i32;
        self.irq_enabled = reader.read_bool()?;
        self.irq_mode = reader.read_bool()?;
        self.irq_pending = reader.read_bool()?;
        self.irq_ack = reader.read_bool()?;
        Ok(())
    }
}
