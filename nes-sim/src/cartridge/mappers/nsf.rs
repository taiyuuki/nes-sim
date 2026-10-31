//! NSF mapper：音乐数据的bank布局 + 扩展音源寄存器路由。
//!
//! 内存模型（按 nesdev NSF 规范）：
//! - `$8000-$FFFF`：8 个 4KB ROM 窗口，`$5FF8-$5FFF` 切换
//! - `$6000-$7FFF`：8KB RAM（非banked且装载地址≥$8000时）
//! - FDS 曲目：`$6000-$DFFF` 为 32KB RAM（FDS 硬件本就是 RAM 区），
//!   `$5FF6/$5FF7` 控制窗口 `$6000/$7000`，bank 切换按「拷贝进 RAM」模型；
//!   头部 bank[6]/[7] 同时初始化 `$6000-$7FFF` 与 `$E000-$FFFF`
//! - MMC5 曲目附带 EXRAM（`$5C00-$5FF5`）与硬件乘法器（`$5205/$5206`）
//! - 扩展芯片寄存器只路由规范指定的最低地址

use std::cell::RefCell;
use std::rc::Rc;

use super::{CartridgeError, Mapper, Mirroring};
use crate::apu::ExpansionAudioChip;
use crate::cartridge::expansion_audio::fds::{FdsAudio, FdsAudioChip};
use crate::cartridge::expansion_audio::mmc5::{Mmc5Audio, Mmc5AudioChip};
use crate::cartridge::expansion_audio::namco163::{Namco163Audio, Namco163AudioChip};
use crate::cartridge::expansion_audio::sunsoft5b::{Sunsoft5bAudio, Sunsoft5bAudioChip};
use crate::cartridge::expansion_audio::vrc6::{Vrc6Audio, Vrc6AudioChip};
use crate::cartridge::expansion_audio::vrc7::{Vrc7Audio, Vrc7AudioChip};
use crate::nsf::NsfHeader;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

/// 窗口未映射（直通wram/开放总线）
const UNMAPPED: usize = usize::MAX;
/// 窗口数：$6000-$F000 共 10 个 4KB 窗口
const WINDOW_COUNT: usize = 10;

pub(crate) struct NsfMapper {
    /// 4KB bank 池（banked时前置装载地址低12位填充）
    banks: Vec<u8>,
    /// 每窗口当前bank（索引0对应$6000窗口）
    window_bank: [usize; WINDOW_COUNT],
    /// 头部初始窗口布局（切曲时恢复）
    initial_window_bank: [usize; WINDOW_COUNT],
    /// 是否启用bank切换
    banked: bool,
    /// FDS曲（$6000-$DFFF为RAM、拷贝式bank）
    fds: bool,
    /// 非FDS: 8KB（$6000-$7FFF）；FDS: 32KB（$6000-$DFFF）
    wram: Vec<u8>,
    /// MMC5 EXRAM（$5C00-$5FF5）
    exram: Box<[u8; 0x400]>,
    // 扩展芯片（与APU共享所有权）
    vrc6: Option<Rc<RefCell<Vrc6Audio>>>,
    vrc7: Option<Rc<RefCell<Vrc7Audio>>>,
    vrc7_latch: u8,
    fds_audio: Option<Rc<RefCell<FdsAudio>>>,
    mmc5: Option<Rc<RefCell<Mmc5Audio>>>,
    /// MMC5乘法器操作数（$5205/$5206）
    mmc5_mult: [u8; 2],
    n163: Option<Rc<RefCell<Namco163Audio>>>,
    s5b: Option<Rc<RefCell<Sunsoft5bAudio>>>,
}

impl NsfMapper {
    fn bank_byte(&self, bank: usize, offset: usize) -> u8 {
        self.banks.get(bank * 0x1000 + offset).copied().unwrap_or(0)
    }

    /// 将bank载入窗口；FDS的RAM窗口按拷贝模型写入wram。
    fn switch_window(&mut self, window: usize, bank: u8) {
        let bank = bank as usize;
        self.window_bank[window] = bank;
        if self.fds && window <= 7 {
            let base = window * 0x1000;
            for i in 0..0x1000 {
                self.wram[base + i] = self.bank_byte(bank, i);
            }
        }
    }

    fn apply_windows(&mut self, windows: &[usize; WINDOW_COUNT]) {
        self.window_bank = *windows;
        if self.fds {
            for (window, &bank) in windows.iter().enumerate().take(8) {
                if bank != UNMAPPED {
                    let base = window * 0x1000;
                    for i in 0..0x1000 {
                        self.wram[base + i] = self.bank_byte(bank, i);
                    }
                }
            }
        }
    }

    fn read_window(&self, addr: u16) -> Option<u8> {
        let window = (addr >> 12) as usize - 6;
        let offset = (addr & 0x0FFF) as usize;
        if self.fds && window <= 7 {
            // FDS：$6000-$DFFF 恒为RAM（bank内容已拷入）
            return Some(self.wram[window * 0x1000 + offset]);
        }
        match self.window_bank[window] {
            UNMAPPED if window <= 1 => Some(self.wram[window * 0x1000 + offset]),
            UNMAPPED => None,
            bank => Some(self.bank_byte(bank, offset)),
        }
    }

    fn write_window(&mut self, addr: u16, data: u8) -> bool {
        let window = (addr >> 12) as usize - 6;
        let offset = (addr & 0x0FFF) as usize;
        if self.fds && window <= 7 {
            self.wram[window * 0x1000 + offset] = data;
            return true;
        }
        if window <= 1 && self.window_bank[window] == UNMAPPED {
            self.wram[window * 0x1000 + offset] = data;
            return true;
        }
        // $8000+ 为ROM，不可写
        false
    }
}

impl Mapper for NsfMapper {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            // FDS音源
            0x4040..=0x407F if self.fds_audio.is_some() => Some(
                self.fds_audio
                    .as_ref()
                    .unwrap()
                    .borrow()
                    .read_wave(addr as usize & 0x3F),
            ),
            0x4090 | 0x4092 if self.fds_audio.is_some() => Some(
                self.fds_audio
                    .as_ref()
                    .unwrap()
                    .borrow()
                    .read_gain(addr as u8),
            ),
            // N163 数据口
            0x4800..=0x4FFF if self.n163.is_some() => {
                Some(self.n163.as_ref().unwrap().borrow_mut().read_ram())
            }
            // MMC5
            0x5015 if self.mmc5.is_some() => Some(self.mmc5.as_ref().unwrap().borrow().status()),
            0x5205 if self.mmc5.is_some() => {
                let product = u16::from(self.mmc5_mult[0]) * u16::from(self.mmc5_mult[1]);
                Some((product & 0xFF) as u8)
            }
            0x5206 if self.mmc5.is_some() => {
                let product = u16::from(self.mmc5_mult[0]) * u16::from(self.mmc5_mult[1]);
                Some((product >> 8) as u8)
            }
            0x5C00..=0x5FF5 if self.mmc5.is_some() => Some(self.exram[(addr - 0x5C00) as usize]),
            0x6000..=0xFFFF => self.read_window(addr),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            // FDS音源寄存器
            0x4040..=0x407F if self.fds_audio.is_some() => {
                self.fds_audio
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .write_wave(addr as usize & 0x3F, data);
                true
            }
            0x4080..=0x408A if self.fds_audio.is_some() => {
                self.fds_audio
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .write_reg((addr - 0x4080) as usize, data);
                true
            }
            // N163
            0x4800..=0x4FFF if self.n163.is_some() => {
                self.n163.as_ref().unwrap().borrow_mut().write_ram(data);
                true
            }
            0xF800..=0xFFFF if self.n163.is_some() => {
                self.n163.as_ref().unwrap().borrow_mut().write_addr(data);
                true
            }
            // MMC5 音源 + 乘法器 + EXRAM
            0x5000..=0x5017 if self.mmc5.is_some() => {
                self.mmc5.as_ref().unwrap().borrow_mut().write(addr, data);
                true
            }
            0x5205 | 0x5206 if self.mmc5.is_some() => {
                self.mmc5_mult[(addr - 0x5205) as usize] = data;
                true
            }
            0x5C00..=0x5FF5 if self.mmc5.is_some() => {
                self.exram[(addr - 0x5C00) as usize] = data;
                true
            }
            // VRC6（规范指定最低地址，不做镜像）
            0x9000..=0x9002 if self.vrc6.is_some() => {
                let mut audio = self.vrc6.as_ref().unwrap().borrow_mut();
                match addr - 0x9000 {
                    0 => audio.pulse1.write_reg0(data),
                    1 => audio.pulse1.write_reg1(data),
                    _ => audio.pulse1.write_reg2(data),
                }
                true
            }
            0xA000..=0xA002 if self.vrc6.is_some() => {
                let mut audio = self.vrc6.as_ref().unwrap().borrow_mut();
                match addr - 0xA000 {
                    0 => audio.pulse2.write_reg0(data),
                    1 => audio.pulse2.write_reg1(data),
                    _ => audio.pulse2.write_reg2(data),
                }
                true
            }
            0xB000..=0xB002 if self.vrc6.is_some() => {
                let mut audio = self.vrc6.as_ref().unwrap().borrow_mut();
                match addr - 0xB000 {
                    0 => audio.saw.write_reg0(data),
                    1 => audio.saw.write_reg1(data),
                    _ => audio.saw.write_reg2(data),
                }
                true
            }
            // VRC7
            0x9010 if self.vrc7.is_some() => {
                self.vrc7_latch = data;
                true
            }
            0x9030 if self.vrc7.is_some() => {
                self.vrc7
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .write(self.vrc7_latch, data);
                true
            }
            // Sunsoft 5B
            0xC000 if self.s5b.is_some() => {
                self.s5b.as_ref().unwrap().borrow_mut().write_address(data);
                true
            }
            0xE000 if self.s5b.is_some() => {
                self.s5b.as_ref().unwrap().borrow_mut().write_data(data);
                true
            }
            // bank切换
            0x5FF6 | 0x5FF7 if self.fds && self.banked => {
                let window = (addr - 0x5FF6) as usize;
                self.switch_window(window, data);
                true
            }
            0x5FF8..=0x5FFF if self.banked => {
                let window = (addr - 0x5FF8 + 2) as usize;
                self.switch_window(window, data);
                true
            }
            0x6000..=0xFFFF => self.write_window(addr, data),
            _ => false,
        }
    }

    fn ppu_read(&mut self, _addr: u16) -> Option<u8> {
        None
    }

    fn ppu_write(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }

    fn mirroring(&self) -> Mirroring {
        // NSF不使用PPU；给一个确定值
        Mirroring::Horizontal
    }

    /// 切曲复位：恢复初始bank窗口、清wram/EXRAM（APU与芯片复位由上层负责）。
    fn nsf_prepare_song(&mut self) {
        self.wram.iter_mut().for_each(|b| *b = 0);
        *self.exram = [0; 0x400];
        self.mmc5_mult = [0; 2];
        self.vrc7_latch = 0;
        let windows = self.initial_window_bank;
        self.apply_windows(&windows);
    }

    fn save_state(&self, writer: &mut StateWriter) {
        for bank in self.window_bank {
            writer.write_u8(if bank == UNMAPPED { 0xFF } else { bank as u8 });
        }
        writer.write_bytes(&self.wram);
        writer.write_bytes(&self.exram[..]);
        writer.write_u8(self.vrc7_latch);
        writer.write_u8(self.mmc5_mult[0]);
        writer.write_u8(self.mmc5_mult[1]);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        for bank in self.window_bank.iter_mut() {
            let raw = reader.read_u8()?;
            *bank = if raw == 0xFF { UNMAPPED } else { raw as usize };
        }
        reader.read_bytes_into(&mut self.wram)?;
        reader.read_bytes_into(&mut self.exram[..])?;
        self.vrc7_latch = reader.read_u8()?;
        self.mmc5_mult[0] = reader.read_u8()?;
        self.mmc5_mult[1] = reader.read_u8()?;
        Ok(())
    }
}

pub(crate) fn new_nsf(
    header: &NsfHeader,
) -> Result<(NsfMapper, Vec<Box<dyn ExpansionAudioChip>>), CartridgeError> {
    if header.data.is_empty() {
        return Err(CartridgeError::InvalidNsf);
    }

    let banked = header.banked();
    let fds = header.chip_flags & 0x04 != 0;

    // bank池：装载地址低12位为前置填充（banked按规范；顺序装载同理，
    // 否则数据相对装载地址错位）
    let sequential_start_window = if banked {
        usize::MAX
    } else {
        (header.load_addr >> 12) as usize - 6
    };
    let mut banks = vec![0u8; header.load_addr as usize & 0x0FFF];
    banks.extend_from_slice(&header.data);
    let rem = banks.len() % 0x1000;
    if rem != 0 {
        banks.resize(banks.len() + (0x1000 - rem), 0);
    }

    // 初始窗口布局
    let mut windows = [UNMAPPED; WINDOW_COUNT];
    if banked {
        for i in 0..8usize {
            let bank = header.banks[i];
            if fds {
                if i < 6 {
                    windows[i + 2] = bank as usize;
                } else {
                    // bank[6]/[7] 同时初始化 $6000-$7FFF 与 $E000-$FFFF
                    windows[i - 6] = bank as usize;
                    windows[i + 2] = bank as usize;
                }
            } else {
                windows[i + 2] = bank as usize;
            }
        }
    } else {
        for (offset, window) in (sequential_start_window..WINDOW_COUNT).enumerate() {
            windows[window] = offset;
        }
    }

    let wram_len = if fds { 0x8000 } else { 0x2000 };

    let mut chips: Vec<Box<dyn ExpansionAudioChip>> = Vec::new();
    let flags = header.chip_flags;

    let vrc6 = if flags & 0x01 != 0 {
        let audio = Rc::new(RefCell::new(Vrc6Audio::new()));
        chips.push(Box::new(Vrc6AudioChip::new(audio.clone())));
        Some(audio)
    } else {
        None
    };
    let vrc7 = if flags & 0x02 != 0 {
        let audio = Rc::new(RefCell::new(Vrc7Audio::new()));
        chips.push(Box::new(Vrc7AudioChip::new(audio.clone())));
        Some(audio)
    } else {
        None
    };
    let fds_audio = if flags & 0x04 != 0 {
        let audio = Rc::new(RefCell::new(FdsAudio::new()));
        chips.push(Box::new(FdsAudioChip::new(audio.clone())));
        Some(audio)
    } else {
        None
    };
    let mmc5 = if flags & 0x08 != 0 {
        let audio = Rc::new(RefCell::new(Mmc5Audio::new()));
        chips.push(Box::new(Mmc5AudioChip::new(audio.clone())));
        Some(audio)
    } else {
        None
    };
    let n163 = if flags & 0x10 != 0 {
        let audio = Rc::new(RefCell::new(Namco163Audio::new()));
        chips.push(Box::new(Namco163AudioChip::new(audio.clone())));
        Some(audio)
    } else {
        None
    };
    let s5b = if flags & 0x20 != 0 {
        let audio = Rc::new(RefCell::new(Sunsoft5bAudio::new()));
        chips.push(Box::new(Sunsoft5bAudioChip::new(audio.clone())));
        Some(audio)
    } else {
        None
    };

    let mut mapper = NsfMapper {
        banks,
        window_bank: windows,
        initial_window_bank: windows,
        banked,
        fds,
        wram: vec![0; wram_len],
        exram: Box::new([0; 0x400]),
        vrc6,
        vrc7,
        vrc7_latch: 0,
        fds_audio,
        mmc5,
        mmc5_mult: [0; 2],
        n163,
        s5b,
    };
    // FDS窗口的初始bank内容拷入wram
    let windows_copy = windows;
    mapper.apply_windows(&windows_copy);

    Ok((mapper, chips))
}
