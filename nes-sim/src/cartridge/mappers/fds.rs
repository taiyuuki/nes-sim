use std::cell::RefCell;
use std::rc::Rc;

use super::Mapper;
use crate::apu::ExpansionAudioChip;
use crate::cartridge::Mirroring;
use crate::cartridge::expansion_audio::fds::{FdsAudio, FdsAudioChip};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

pub(super) const FDS_SIDE_SIZE: usize = 65500;
const BIOS_SIZE: usize = 8192;
const WRAM_SIZE: usize = 0x8000;

// 每字节传输延迟（CPU周期）；fceux为150，nestopia精确值为148
const DISK_BYTE_CYCLES: i32 = 150;

// 磁盘块链ID，与$4025 bit6马达沿推进顺序一致
const BLOCK_INIT: u8 = 0;
const BLOCK_VOLUME: u8 = 1;
const BLOCK_FILECNT: u8 = 2;
const BLOCK_FILEHDR: u8 = 3;
const BLOCK_FILEDATA: u8 = 4;

/// FDS适配器：BIOS($E000-$FFFF) + 32KB WRAM($6000-$DFFF) + 磁盘驱动器寄存器
/// ($4020-$4033) + 扩展音频($4040-$4092)。
/// 驱动器采用fceux的块链模型：镜像中的gap/CRC字节原样流经$4031，
/// 由真实的DISKSYS.ROM BIOS完成块解析与载入。
pub(super) struct Fds {
    bios: Vec<u8>,
    // Box避免MapperEnum体积膨胀(32KB内联会放大调用链栈帧导致溢出)
    wram: Box<[u8; WRAM_SIZE]>,
    sides: Vec<Vec<u8>>,
    disk_written: bool,
    // $4020-$4026 寄存器镜像
    regs: [u8; 7],
    // 16位重载IRQ定时器
    irq_latch: u16,
    irq_count: i32,
    irq_enabled: bool,
    irq_repeat: bool,
    timer_irq: bool,
    transfer_irq: bool,
    // 字节传输完成锁存（$4030 bit1）。与IRQ使能($4025 bit7)解耦:
    // 轮询模式的BIOS载入不开启传输IRQ，但仍需通过$4030查询字节就绪
    transfer_flag: bool,
    // 驱动器块链状态（fceux mapperFDS_*）
    control: u8,
    block: u8,
    block_start: usize,
    block_len: usize,
    disk_addr: usize,
    file_size: u16,
    disk_access: bool,
    disk_seek_irq: i32,
    // 当前插入面/待选面（None = 退盘）
    selected_disk: usize,
    in_disk: Option<usize>,
    audio: Rc<RefCell<FdsAudio>>,
}

impl Fds {
    pub(super) fn new(
        bios: Vec<u8>,
        sides: Vec<Vec<u8>>,
    ) -> (Self, Vec<Box<dyn ExpansionAudioChip>>) {
        debug_assert_eq!(bios.len(), BIOS_SIZE);
        let audio = Rc::new(RefCell::new(FdsAudio::new()));
        let mapper = Self {
            bios,
            wram: Box::new([0; WRAM_SIZE]),
            sides,
            disk_written: false,
            regs: [0; 7],
            irq_latch: 0,
            irq_count: 0,
            irq_enabled: false,
            irq_repeat: false,
            timer_irq: false,
            transfer_irq: false,
            transfer_flag: false,
            control: 0,
            block: BLOCK_INIT,
            block_start: 0,
            block_len: 0,
            disk_addr: 0,
            file_size: 0,
            disk_access: false,
            disk_seek_irq: 0,
            selected_disk: 0,
            in_disk: Some(0),
            audio: Rc::clone(&audio),
        };
        (mapper, vec![Box::new(FdsAudioChip::new(audio))])
    }

    fn disk_byte(&self) -> u8 {
        match self.in_disk {
            Some(side) => self.sides[side]
                .get(self.block_start + self.disk_addr)
                .copied()
                .unwrap_or(0xFF),
            None => 0xFF,
        }
    }

    fn write_disk_byte(&mut self, data: u8) {
        if let Some(side) = self.in_disk {
            let pos = self.block_start + self.disk_addr;
            if pos < self.sides[side].len() {
                self.sides[side][pos] = data;
                self.disk_written = true;
            }
        }
    }

    fn read_disk_data(&mut self) -> u8 {
        // 未插盘或非读模式返回开放总线值0xFF
        if self.in_disk.is_none() || self.control & 0x04 == 0 {
            return 0xFF;
        }
        self.disk_access = true;
        let mut result = 0;
        if self.disk_addr < self.block_len {
            result = self.disk_byte();
            if self.block == BLOCK_FILEHDR {
                match self.disk_addr {
                    13 => self.file_size = u16::from(result),
                    14 => self.file_size |= u16::from(result) << 8,
                    _ => {}
                }
            }
            self.disk_addr += 1;
        }
        self.disk_seek_irq = DISK_BYTE_CYCLES;
        self.transfer_irq = false;
        self.transfer_flag = false;
        result
    }

    fn write_disk_data(&mut self, data: u8) {
        if self.in_disk.is_none() || self.control & 0x04 != 0 {
            return;
        }
        // 写入启动一次字节传输: 150周期后完成标志置位
        self.disk_seek_irq = DISK_BYTE_CYCLES;
        self.transfer_flag = false;
        self.transfer_irq = false;
        // 马达/模式切换后的首个写入对应CRC字节槽，丢弃
        if !self.disk_access {
            self.disk_access = true;
            return;
        }
        if self.disk_addr < self.block_len {
            self.write_disk_byte(data);
            if self.block == BLOCK_FILEHDR {
                match self.disk_addr {
                    13 => self.file_size = u16::from(data),
                    14 => self.file_size |= u16::from(data) << 8,
                    _ => {}
                }
            }
            self.disk_addr += 1;
        }
    }

    fn write_control(&mut self, data: u8) {
        self.transfer_irq = false;
        // nestopia语义: 传输完成标志仅在新的bit7置位时保留
        if data & 0x80 == 0 {
            self.transfer_flag = false;
        }
        if self.in_disk.is_some() {
            // 马达开启沿：推进到下一块并预置块长
            if data & 0x40 != 0 && self.control & 0x40 == 0 {
                self.disk_access = false;
                self.disk_seek_irq = DISK_BYTE_CYCLES;
                self.block_start += self.disk_addr;
                self.disk_addr = 0;
                self.block += 1;
                if self.block > BLOCK_FILEDATA {
                    self.block = BLOCK_FILEHDR;
                }
                self.block_len = match self.block {
                    BLOCK_VOLUME => 0x38,
                    BLOCK_FILECNT => 0x02,
                    BLOCK_FILEHDR => 0x10,
                    BLOCK_FILEDATA => 0x01 + self.file_size as usize,
                    _ => 0,
                };
            }
            if data & 0x02 != 0 {
                // 传输重置：块链回到起点
                self.block = BLOCK_INIT;
                self.block_start = 0;
                self.block_len = 0;
                self.disk_addr = 0;
                self.disk_seek_irq = DISK_BYTE_CYCLES;
            }
            if data & 0x40 != 0 {
                self.disk_seek_irq = DISK_BYTE_CYCLES;
            }
        }
        self.control = data;
        self.regs[5] = data;
    }

    fn read_status_4030(&mut self) -> u8 {
        // bit3回显$4025 bit3（镜像方式），硬件实测行为
        let mut result = self.control & 0x08;
        if self.timer_irq {
            result |= 0x01;
        }
        if self.transfer_flag || self.transfer_irq {
            result |= 0x02;
        }
        self.timer_irq = false;
        self.transfer_irq = false;
        self.transfer_flag = false;
        result
    }

    fn read_drive_status_4032(&self) -> u8 {
        let mut result = 0x40; // 开放总线位
        let inserted = self.in_disk.is_some();
        if !inserted {
            result |= 0x05;
        }
        // bit1=0表示就绪：需要插盘、马达开、非传输重置
        if !inserted || self.regs[5] & 0x01 == 0 || self.regs[5] & 0x02 != 0 {
            result |= 0x02;
        }
        result
    }
}

impl Mapper for Fds {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x4030 => Some(self.read_status_4030()),
            0x4031 => Some(self.read_disk_data()),
            0x4032 => Some(self.read_drive_status_4032()),
            0x4033 => Some(0x80), // 电池状态
            0x4040..=0x407F => Some(self.audio.borrow().read_wave(addr as usize & 0x3F)),
            0x4090 | 0x4092 => Some(self.audio.borrow().read_gain(addr as u8)),
            0x6000..=0xDFFF => Some(self.wram[(addr - 0x6000) as usize]),
            0xE000..=0xFFFF => Some(self.bios[(addr - 0xE000) as usize]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x4020 => self.irq_latch = (self.irq_latch & 0xFF00) | u16::from(data),
            0x4021 => self.irq_latch = (self.irq_latch & 0x00FF) | (u16::from(data) << 8),
            0x4022 => {
                // 仅当$4023 bit0（磁盘/定时器使能）置位时接受
                if self.regs[3] & 0x01 != 0 {
                    self.irq_repeat = data & 0x01 != 0;
                    self.irq_enabled = data & 0x02 != 0;
                    if self.irq_enabled {
                        self.irq_count = i32::from(self.irq_latch);
                    } else {
                        self.timer_irq = false;
                    }
                }
            }
            0x4023 => {
                if data & 0x01 == 0 {
                    self.irq_enabled = false;
                    self.timer_irq = false;
                    self.transfer_irq = false;
                }
            }
            0x4024 => self.write_disk_data(data),
            0x4025 => self.write_control(data),
            0x4026 => {} // 扩展连接器，无外设
            0x4040..=0x407F => self
                .audio
                .borrow_mut()
                .write_wave(addr as usize & 0x3F, data),
            0x4080..=0x408A => self
                .audio
                .borrow_mut()
                .write_reg((addr - 0x4080) as usize, data),
            0x6000..=0xDFFF => self.wram[(addr - 0x6000) as usize] = data,
            _ => return false,
        }
        if matches!(addr, 0x4020..=0x4026) {
            self.regs[(addr - 0x4020) as usize] = data;
        }
        true
    }

    fn ppu_read(&mut self, _addr: u16) -> Option<u8> {
        None
    }

    fn ppu_write(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }

    fn mirroring(&self) -> Mirroring {
        // $4025 bit3：1=水平，0=垂直（运行时可切换）
        if self.control & 0x08 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn irq_line(&self) -> bool {
        self.timer_irq || self.transfer_irq
    }

    fn tick_cpu_cycle(&mut self) {
        if self.irq_enabled {
            self.irq_count -= 1;
            if self.irq_count <= 0 {
                self.irq_count = i32::from(self.irq_latch);
                self.timer_irq = true;
                if !self.irq_repeat {
                    self.irq_enabled = false;
                }
            }
        }
        if self.disk_seek_irq > 0 {
            self.disk_seek_irq -= 1;
            if self.disk_seek_irq <= 0 {
                // 马达运转期间驱动器持续按固定节奏送出字节(nestopia语义)，
                // 纯轮询的载入器不依赖$4031读回读重新武装计数器
                let consumed = !self.transfer_flag;
                self.transfer_flag = true;
                // IRQ仅在上一字节已被消费时继续触发，避免无人处理的IRQ风暴
                if self.regs[5] & 0x80 != 0 && consumed {
                    self.transfer_irq = true;
                }
                if self.in_disk.is_some() && self.control & 0x01 != 0 {
                    self.disk_seek_irq = DISK_BYTE_CYCLES;
                }
            }
        }
    }

    fn fds_command(&mut self, cmd: super::FdsDiskCommand) {
        match cmd {
            super::FdsDiskCommand::ToggleInsert => {
                self.in_disk = if self.in_disk.is_some() {
                    None
                } else {
                    Some(self.selected_disk.min(self.sides.len() - 1))
                };
            }
            super::FdsDiskCommand::SelectNextSide => {
                // fceux要求退盘状态下才能换面
                if self.in_disk.is_none() && !self.sides.is_empty() {
                    self.selected_disk = (self.selected_disk + 1) % self.sides.len();
                }
            }
        }
    }

    fn fds_info(&self) -> Option<super::FdsDiskInfo> {
        Some(super::FdsDiskInfo {
            side_count: self.sides.len(),
            selected_side: self.selected_disk,
            inserted: self.in_disk.is_some(),
        })
    }

    fn fds_dirty(&self) -> bool {
        self.disk_written
    }

    fn fds_sides(&self) -> Option<&[Vec<u8>]> {
        Some(&self.sides)
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u32(self.sides.len() as u32);
        for side in &self.sides {
            writer.write_bytes(side);
        }
        writer.write_bytes(self.wram.as_slice());
        writer.write_bytes(&self.regs);
        writer.write_u16(self.irq_latch);
        writer.write_u32(self.irq_count as u32);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_repeat);
        writer.write_bool(self.timer_irq);
        writer.write_bool(self.transfer_irq);
        writer.write_bool(self.transfer_flag);
        writer.write_u8(self.control);
        writer.write_u8(self.block);
        writer.write_u32(self.block_start as u32);
        writer.write_u32(self.block_len as u32);
        writer.write_u32(self.disk_addr as u32);
        writer.write_u16(self.file_size);
        writer.write_bool(self.disk_access);
        writer.write_u32(self.disk_seek_irq as u32);
        writer.write_u32(self.selected_disk as u32);
        writer.write_bool(self.in_disk.is_some());
        writer.write_u8(self.in_disk.unwrap_or(0) as u8);
        writer.write_bool(self.disk_written);
        self.audio.borrow().save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        let side_count = reader.read_u32()? as usize;
        if side_count != self.sides.len() {
            return Err(SaveStateError::InvalidData("FDS side count mismatch"));
        }
        for side in &mut self.sides {
            reader.read_bytes_into(side)?;
        }
        reader.read_bytes_into(self.wram.as_mut_slice())?;
        reader.read_bytes_into(&mut self.regs)?;
        self.irq_latch = reader.read_u16()?;
        self.irq_count = reader.read_u32()? as i32;
        self.irq_enabled = reader.read_bool()?;
        self.irq_repeat = reader.read_bool()?;
        self.timer_irq = reader.read_bool()?;
        self.transfer_irq = reader.read_bool()?;
        self.transfer_flag = reader.read_bool()?;
        self.control = reader.read_u8()?;
        self.block = reader.read_u8()?;
        self.block_start = reader.read_u32()? as usize;
        self.block_len = reader.read_u32()? as usize;
        self.disk_addr = reader.read_u32()? as usize;
        self.file_size = reader.read_u16()?;
        self.disk_access = reader.read_bool()?;
        self.disk_seek_irq = reader.read_u32()? as i32;
        self.selected_disk = reader.read_u32()? as usize;
        let inserted = reader.read_bool()?;
        let side = reader.read_u8()? as usize;
        self.in_disk = if inserted { Some(side) } else { None };
        self.disk_written = reader.read_bool()?;
        self.audio.borrow_mut().load_state(reader)?;
        Ok(())
    }
}

/// 解析.fds镜像（fwNES 16字节头或raw无头格式）为每面65500字节的列表。
pub(crate) fn parse_fds_sides(image: &[u8]) -> Result<Vec<Vec<u8>>, super::CartridgeError> {
    let (side_count, data_start) = if image.len() >= 4 && &image[0..4] == b"FDS\x1a" {
        (image[4] as usize, 16)
    } else if image.len() >= 15 && &image[1..15] == b"*NINTENDO-HVC*" {
        (image.len().max(FDS_SIDE_SIZE) / FDS_SIDE_SIZE, 0)
    } else {
        return Err(super::CartridgeError::InvalidFds);
    };

    let side_count = side_count.clamp(1, 8);
    let mut sides = Vec::with_capacity(side_count);
    for index in 0..side_count {
        let start = data_start + index * FDS_SIDE_SIZE;
        let mut side = vec![0u8; FDS_SIDE_SIZE];
        let end = (start + FDS_SIDE_SIZE).min(image.len());
        if start < end {
            side[..end - start].copy_from_slice(&image[start..end]);
        }
        sides.push(side);
    }
    Ok(sides)
}

pub(crate) fn new_fds(
    bios: Vec<u8>,
    sides: Vec<Vec<u8>>,
) -> Result<(super::MapperEnum, Vec<Box<dyn ExpansionAudioChip>>), super::CartridgeError> {
    if bios.len() != BIOS_SIZE {
        return Err(super::CartridgeError::FdsBiosInvalid);
    }
    let (fds, chips) = Fds::new(bios, sides);
    Ok((super::MapperEnum::Fds(fds), chips))
}
