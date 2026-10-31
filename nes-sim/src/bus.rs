use crate::apu::APU;
use crate::cartridge::{
    Cartridge, CartridgeError, FdsDiskCommand, FdsDiskInfo, Mirroring, TVSystem, is_fds_image,
};
use crate::cheat::{Cheat, CheatTable};
use crate::dma::{DmaBusRequest, DmaController};
use crate::input::{ControllerState, Joypad};
use crate::ppu::PPU;
#[cfg(feature = "debug")]
use crate::ppu::PPUBus;
use crate::ppu_memory::PpuMemory;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

#[cfg(feature = "debug")]
use crate::api::{Breakpoint, MemorySnapshot};

pub trait CPUBus {
    fn cpu_read(&mut self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, data: u8);
    fn cpu_read_timed(&mut self, addr: u16, cycle_offset: u8) -> u8 {
        let _ = cycle_offset;
        self.cpu_read(addr)
    }
    fn cpu_write_timed(&mut self, addr: u16, data: u8, cycle_offset: u8) {
        let _ = cycle_offset;
        self.cpu_write(addr, data);
    }
    fn try_dma(&mut self) -> bool {
        false
    }

    fn cpu_read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.cpu_read(addr) as u16;
        let hi = self.cpu_read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }
}

pub struct NESBus {
    pub ram: [u8; 0x800],
    ppu: PPU,
    ppu_memory: PpuMemory,
    apu: APU,
    dma: DmaController,
    controllers: [Joypad; 2],
    cpu_open_bus: u8,
    cheats: Vec<Cheat>,
    cheat_table: CheatTable,
    /// NSF模式：屏蔽$2000-$3FFF的PPU访问（NSF不产生画面，防止误触发NMI）
    nsf_mode: bool,
    #[cfg(feature = "debug")]
    debug_mem_breakpoints: Vec<Breakpoint>,
    #[cfg(feature = "debug")]
    debug_mem_breakpoint_hit: Option<Breakpoint>,
    // Additional components: APU, cartridge, etc. can be added here
}

impl NESBus {
    pub fn new() -> Self {
        NESBus {
            ram: [0; 0x800],
            ppu: PPU::new(),
            ppu_memory: PpuMemory::new(),
            apu: APU::new(),
            dma: DmaController::new(),
            controllers: [Joypad::new(), Joypad::new()],
            cpu_open_bus: 0,
            cheats: Vec::new(),
            cheat_table: CheatTable::default(),
            nsf_mode: false,
            #[cfg(feature = "debug")]
            debug_mem_breakpoints: Vec::new(),
            #[cfg(feature = "debug")]
            debug_mem_breakpoint_hit: None,
        }
    }

    pub fn ppu(&self) -> &PPU {
        &self.ppu
    }

    pub fn add_expansion_audio_chip(&mut self, chip: Box<dyn crate::apu::ExpansionAudioChip>) {
        self.apu.add_expansion_chip(chip);
    }

    pub fn set_controller_state(&mut self, port: usize, state: ControllerState) {
        if let Some(controller) = self.controllers.get_mut(port) {
            controller.set_state(state);
        }
    }

    /// 当前端口手柄输入状态（录像机/调试用）；端口不存在时返回全松开。
    pub fn controller_state(&self, port: usize) -> ControllerState {
        self.controllers
            .get(port)
            .map(|controller| controller.state())
            .unwrap_or_default()
    }

    pub fn insert_cartridge(&mut self, mut cartridge: Cartridge) {
        // 金手指属于当前游戏，换卡时清空。
        self.clear_cheats();
        let tv_system = cartridge.tv_system();
        self.ppu.set_parameters(tv_system);
        self.apu.set_tv_system(tv_system);
        // 上一张卡的扩展芯片不再生效
        self.apu.clear_expansion_chips();
        self.nsf_mode = cartridge.is_nsf();
        let chips = cartridge.take_expansion_audio_chips();
        self.ppu_memory.insert_cartridge(cartridge);
        for chip in chips {
            self.apu.add_expansion_chip(chip);
        }
    }

    /// NSF切曲准备：清CPU RAM、恢复初始bank窗口、清wram/EXRAM、
    /// 复位APU 2A03与扩展芯片并写入规范要求的初始寄存器序列。
    pub fn nsf_prepare_song(&mut self, fds_chip: bool) {
        self.ram = [0; 0x800];
        self.ppu_memory.cartridge_nsf_prepare_song();
        self.apu.reset();
        self.apu.reset_expansion_chips();
        // 规范初始序列：$4000-$4013清零、$4015=$00→$0F、$4017=$40
        for addr in 0x4000..=0x4013 {
            self.cpu_write_internal(addr, 0x00, 0);
        }
        self.cpu_write_internal(0x4015, 0x00, 0);
        self.cpu_write_internal(0x4015, 0x0F, 0);
        self.cpu_write_internal(0x4017, 0x40, 0);
        if fds_chip {
            // FDS惯例初始：音量/包络（FCEUX口径）
            self.cpu_write_internal(0x4089, 0x80, 0);
            self.cpu_write_internal(0x408A, 0xE8, 0);
        }
    }

    pub fn load_cartridge_ines(&mut self, rom: &[u8]) -> Result<(), CartridgeError> {
        let cartridge: Cartridge = Cartridge::from_ines(rom)?;
        self.insert_cartridge(cartridge);
        Ok(())
    }

    /// 按格式嗅探加载ROM；FDS镜像需要额外的BIOS ROM数据。
    pub fn load_rom(&mut self, rom: &[u8], bios: Option<&[u8]>) -> Result<(), CartridgeError> {
        if crate::nsf::is_nsf_image(rom) {
            let header = crate::nsf::parse_nsf(rom)?;
            let cartridge = Cartridge::from_nsf(&header)?;
            self.insert_cartridge(cartridge);
            Ok(())
        } else if is_fds_image(rom) {
            let bios = bios.ok_or(CartridgeError::FdsBiosMissing)?;
            let cartridge = Cartridge::from_fds(rom, bios)?;
            self.insert_cartridge(cartridge);
            Ok(())
        } else {
            self.load_cartridge_ines(rom)
        }
    }

    pub fn fds_command(&mut self, cmd: FdsDiskCommand) {
        self.ppu_memory.fds_command(cmd)
    }

    pub fn fds_info(&self) -> Option<FdsDiskInfo> {
        self.ppu_memory.fds_info()
    }

    pub fn fds_dirty(&self) -> bool {
        self.ppu_memory.fds_dirty()
    }

    pub fn fds_sides(&self) -> Option<&[Vec<u8>]> {
        self.ppu_memory.fds_sides()
    }

    /// 电池后备PRG RAM；仅头部声明电池标志的卡带返回Some。
    pub fn battery_ram(&self) -> Option<&[u8]> {
        self.ppu_memory.battery_ram()
    }

    /// 载入电池后备PRG RAM。
    pub fn load_battery_ram(&mut self, data: &[u8]) {
        self.ppu_memory.load_battery_ram(data)
    }

    pub fn reset(&mut self) {
        self.ppu.reset();
        self.apu.reset();
        self.dma = DmaController::new();
        self.cpu_open_bus = 0;
        // 金手指是运行时配置，reset 不清除。
    }

    /// 金手指主列表（含未启用项，按添加顺序）。
    pub fn cheats(&self) -> &[Cheat] {
        &self.cheats
    }

    /// 整体替换金手指列表（加载 .cht 文件用）。
    pub fn set_cheats(&mut self, cheats: Vec<Cheat>) {
        self.cheats = cheats;
        self.cheat_table.rebuild(&self.cheats);
    }

    pub fn add_cheat(&mut self, cheat: Cheat) {
        self.cheats.push(cheat);
        self.cheat_table.rebuild(&self.cheats);
    }

    /// 按下标移除；返回是否存在并被移除。
    pub fn remove_cheat(&mut self, index: usize) -> bool {
        if index < self.cheats.len() {
            self.cheats.remove(index);
            self.cheat_table.rebuild(&self.cheats);
            true
        } else {
            false
        }
    }

    /// 启/禁用某条金手指；返回是否存在并被修改。
    pub fn set_cheat_enabled(&mut self, index: usize, enabled: bool) -> bool {
        if let Some(cheat) = self.cheats.get_mut(index) {
            cheat.enabled = enabled;
            self.cheat_table.rebuild(&self.cheats);
            true
        } else {
            false
        }
    }

    pub fn clear_cheats(&mut self) {
        self.cheats.clear();
        self.cheat_table.rebuild(&self.cheats);
    }

    pub fn tick_ppu(&mut self) {
        let ppu = &mut self.ppu;
        let ppu_memory = &mut self.ppu_memory;
        ppu.clock(ppu_memory);
    }

    pub fn ppu_nmi_line(&self) -> bool {
        self.ppu.nmi_line()
    }

    pub fn ppu_frame(&self) -> u64 {
        self.ppu.frame()
    }

    pub fn tick_apu_cpu_cycle(&mut self) {
        self.apu.tick_cpu_cycle();
        self.ppu_memory.cartridge_tick_cpu_cycle();
    }

    pub fn apu_sample_rate(&self) -> u32 {
        self.apu.sample_rate()
    }

    pub fn set_apu_sample_rate(&mut self, sample_rate: u32) {
        self.apu.set_sample_rate(sample_rate);
    }

    pub fn apu_audio_samples(&self) -> &[f32] {
        self.apu.audio_samples()
    }

    pub fn clear_apu_audio_samples(&mut self) {
        self.apu.clear_audio_samples();
    }

    pub fn set_apu_debug_mute_mask(&mut self, mask: u8) {
        self.apu.set_debug_mute_mask(mask);
    }

    pub fn apu_debug_mute_mask(&self) -> u8 {
        self.apu.debug_mute_mask()
    }

    pub fn apu_irq_line(&self) -> bool {
        self.apu.irq_line()
    }

    /// 上电起累计的CPU周期数（含NSF挂起期间；播放计时用）。
    pub fn apu_cycle_count(&self) -> u64 {
        self.apu.cycle_count()
    }

    pub fn cartridge_irq_line(&self) -> bool {
        self.ppu_memory.cartridge_irq_line()
    }

    /// 当前卡带 PRG‖CHR 的 MD5（无卡/FDS 为 None）。fm2 `romChecksum` 口径。
    pub fn rom_md5(&self) -> Option<[u8; 16]> {
        self.ppu_memory.cartridge_rom_md5()
    }

    /// 当前卡带 PRG‖CHR 的 MD5 十六进制字符串。
    pub fn rom_md5_hex(&self) -> Option<String> {
        self.rom_md5().map(|digest| crate::hash::to_hex(&digest))
    }

    /// 当前卡带 PRG‖CHR 的 CRC-32（无卡/FDS 为 None）。Nestopia .cht `game` 口径。
    pub fn rom_crc32(&self) -> Option<u32> {
        self.ppu_memory.cartridge_rom_crc32()
    }

    pub fn mirroring(&self) -> Mirroring {
        self.ppu_memory.mirroring()
    }

    #[allow(dead_code)]
    pub fn dma_in_progress(&self) -> bool {
        self.dma.in_progress()
    }

    pub fn advance_dma_cpu_phase(&mut self) {
        self.dma.advance_cpu_phase();
    }

    #[cfg(feature = "debug")]
    pub fn debug_read_chr(&mut self) -> [u8; 0x2000] {
        let mut chr = [0u8; 0x2000];
        for (i, byte) in chr.iter_mut().enumerate() {
            *byte = self.ppu_memory.ppu_read(i as u16);
        }
        chr
    }

    #[cfg(feature = "debug")]
    pub fn debug_memory_snapshot(&self) -> MemorySnapshot<'_> {
        MemorySnapshot {
            ram: &self.ram,
            vram: self.ppu_memory.debug_vram_snapshot(),
            chr: self.ppu_memory.debug_chr_snapshot(),
            palette: self.ppu_memory.debug_palette_snapshot(),
            oam: self.ppu.debug_oam_snapshot(),
        }
    }

    #[cfg(feature = "debug")]
    pub fn set_debug_mem_breakpoints(&mut self, breakpoints: &[Breakpoint]) {
        self.debug_mem_breakpoints.clear();
        self.debug_mem_breakpoints.extend_from_slice(breakpoints);
        self.debug_mem_breakpoint_hit = None;
    }

    #[cfg(feature = "debug")]
    pub fn take_mem_breakpoint_hit(&mut self) -> Option<Breakpoint> {
        self.debug_mem_breakpoint_hit.take()
    }

    #[cfg(feature = "debug")]
    fn check_mem_breakpoint(&mut self, addr: u16, is_write: bool) {
        if self.debug_mem_breakpoint_hit.is_some() {
            return;
        }
        for &bp in &self.debug_mem_breakpoints {
            match bp {
                Breakpoint::MemoryRead(target) if !is_write && addr == target => {
                    self.debug_mem_breakpoint_hit = Some(bp);
                    return;
                }
                Breakpoint::MemoryWrite(target) if is_write && addr == target => {
                    self.debug_mem_breakpoint_hit = Some(bp);
                    return;
                }
                _ => {}
            }
        }
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) -> Result<(), SaveStateError> {
        writer.write_bytes(&self.ram);
        self.ppu.save_state(writer);
        self.ppu_memory.save_state(writer)?;
        self.apu.save_state(writer);
        self.dma.save_state(writer);
        for controller in &self.controllers {
            controller.save_state(writer);
        }
        writer.write_u8(self.cpu_open_bus);
        Ok(())
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.ram)?;
        self.ppu.load_state(reader)?;
        self.ppu_memory.load_state(reader)?;
        self.apu.load_state(reader)?;
        self.dma.load_state(reader)?;
        for controller in &mut self.controllers {
            controller.load_state(reader)?;
        }
        self.cpu_open_bus = reader.read_u8()?;
        // APU 存档不含制式，从 PPU 恢复的制式同步帧序列器/周期表/采样率主频
        self.apu.set_tv_system(self.ppu.tv_system());
        Ok(())
    }

    /// 覆盖制式（桌面端 --tv-system 等）：换卡后以新卡带头部为准。
    pub(crate) fn set_tv_system(&mut self, tv: TVSystem) {
        self.ppu.set_parameters(tv);
        self.apu.set_tv_system(tv);
    }

    fn latched_cpu_read(&mut self, data: u8) -> u8 {
        self.cpu_open_bus = data;
        data
    }

    fn cpu_read_internal(&mut self, addr: u16, cycle_offset: u8) -> u8 {
        if self.cheat_table.is_active() {
            let raw = self.cpu_read_raw(addr, cycle_offset);
            match self.cheat_table.substitute(addr, raw) {
                Some(substituted) => self.latched_cpu_read(substituted),
                None => raw,
            }
        } else {
            self.cpu_read_raw(addr, cycle_offset)
        }
    }

    fn cpu_read_raw(&mut self, addr: u16, cycle_offset: u8) -> u8 {
        #[cfg(feature = "debug")]
        self.check_mem_breakpoint(addr, false);

        match addr {
            0x0000..=0x1FFF => self.latched_cpu_read(self.ram[(addr & 0x7FF) as usize]),
            // NSF模式屏蔽PPU寄存器（读开放总线）
            0x2000..=0x3FFF if self.nsf_mode => self.latched_cpu_read(self.cpu_open_bus),
            0x2000..=0x3FFF => {
                let ppu = &mut self.ppu;
                let ppu_memory = &mut self.ppu_memory;
                let data =
                    ppu.cpu_read_register_timed(ppu_memory, 0x2000 | (addr & 0x0007), cycle_offset);
                self.latched_cpu_read(data)
            }
            0x4015 => {
                let data =
                    self.apu.read_status_at_offset(cycle_offset) | (self.cpu_open_bus & 0x20);
                self.latched_cpu_read(data)
            }
            0x4016 => {
                let data = (self.cpu_open_bus & 0xE0) | self.controllers[0].read();
                self.latched_cpu_read(data)
            }
            0x4017 => {
                let data = (self.cpu_open_bus & 0xE0) | self.controllers[1].read();
                self.latched_cpu_read(data)
            }
            0x4020..=0xFFFF => {
                let data = self
                    .ppu_memory
                    .cartridge_cpu_read(addr)
                    .unwrap_or(self.cpu_open_bus);
                self.latched_cpu_read(data)
            }
            // Handle other address ranges (APU, cartridge, etc.)
            _ => self.cpu_open_bus,
        }
    }

    fn cpu_write_internal(&mut self, addr: u16, data: u8, cycle_offset: u8) {
        self.cpu_open_bus = data;
        #[cfg(feature = "debug")]
        self.check_mem_breakpoint(addr, true);
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x7FF) as usize] = data,
            // NSF模式屏蔽PPU寄存器（写忽略）
            0x2000..=0x3FFF if self.nsf_mode => {}
            0x2000..=0x3FFF => {
                let ppu = &mut self.ppu;
                let ppu_memory = &mut self.ppu_memory;
                ppu.cpu_write_register_timed(
                    ppu_memory,
                    0x2000 | (addr & 0x0007),
                    data,
                    cycle_offset,
                );
            }
            0x4014 => self.dma.request_oam_dma(data),
            0x4000..=0x4013 | 0x4015 => self.apu.write_register_at_offset(addr, data, cycle_offset),
            0x4016 => {
                self.controllers[0].write(data);
                self.controllers[1].write(data);
            }
            0x4017 => self.apu.write_register_at_offset(addr, data, cycle_offset),
            0x4020..=0xFFFF => {
                let _ = self.ppu_memory.cartridge_cpu_write(addr, data);
            }
            // Handle other address ranges (APU, cartridge, etc.)
            _ => {}
        }
    }

    fn tick_dma_cpu_cycle(&mut self) -> bool {
        let dmc_request = self.apu.take_dmc_dma_request();
        let (consumed, request) = self.dma.tick_cpu_cycle(dmc_request);
        match request {
            DmaBusRequest::None => {}
            DmaBusRequest::DmcRead { addr } => {
                let data = self.cpu_read_internal(addr, 0);
                self.apu.submit_dmc_dma_sample(data);
                self.dma.apply_dmc_read();
            }
            DmaBusRequest::OamRead { addr } => {
                let data = self.cpu_read_internal(addr, 0);
                self.dma.apply_oam_read(data);
            }
            DmaBusRequest::OamWrite { data } => {
                self.ppu.write_oam_dma(data);
                self.dma.apply_oam_write();
            }
        }
        consumed
    }
}

impl CPUBus for NESBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.cpu_read_internal(addr, 0)
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        self.cpu_write_internal(addr, data, 0);
    }

    fn cpu_read_timed(&mut self, addr: u16, cycle_offset: u8) -> u8 {
        self.cpu_read_internal(addr, cycle_offset)
    }

    fn cpu_write_timed(&mut self, addr: u16, data: u8, cycle_offset: u8) {
        self.cpu_write_internal(addr, data, cycle_offset);
    }

    fn try_dma(&mut self) -> bool {
        self.tick_dma_cpu_cycle()
    }
}

impl Default for NESBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
