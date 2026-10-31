pub mod api;
mod apu;
mod bus;
pub mod cartridge;
pub mod cheat;
mod cpu;
mod dma;
pub mod hash;
pub mod headless;
mod input;
pub mod movie;
mod nsf;
mod ppu;
mod ppu_memory;
pub mod runtime;
pub mod savestate;
pub mod video;

pub use api::{
    AudioBatch, CoreCommand, CoreEvent, CoreResponse, CpuDebugSnapshot, DebugSnapshot, PixelFormat,
    PpuDebugSnapshot, VIDEO_FRAME_PITCH, VideoFrame,
};
#[cfg(feature = "debug")]
pub use api::{Breakpoint, Debugger, DisassembledInstruction, DisassemblyResult, MemorySnapshot};
pub use apu::ExpansionAudioChip;
pub use cartridge::{
    Cartridge, CartridgeError, FdsDiskCommand, FdsDiskInfo, Mirroring, TVSystem, is_fds_image,
};
pub use cheat::{Cheat, CheatError};
pub use input::{ControllerButton, ControllerState};
pub use movie::{Movie, MovieError, MovieFrame};
pub use nsf::{NsfInfo, is_nsf_image};
pub use ppu::{FRAME_HEIGHT, FRAME_WIDTH};
pub use runtime::{
    ExecutionTarget, FrontendInput, FrontendRuntime, RunMode, RuntimeSnapshot, RuntimeStatus,
};
pub use savestate::SaveStateError;
use savestate::{StateReader, StateWriter};

const PAL_CPU_SCHEDULE: [u8; 5] = [3, 3, 3, 3, 4];

/// NSF INIT例程的CPU周期预算（约10帧；正常INIT远小于此）。
const NSF_INIT_BUDGET_CYCLES: u64 = 300_000;

/// 电影播放器状态（运行时配置，不参与存档）。
struct MoviePlayer {
    movie: Movie,
    frame: usize,
    finished: bool,
}

/// 电影录像机状态。
struct MovieRecorder {
    movie: Movie,
    pending_reset: bool,
}

pub struct NES {
    cpu: cpu::CPU,
    bus: bus::NESBus,
    master_clock: u64,
    cpu_ppu_counter: u8,
    cpu_schedule_index: usize,
    cached_tv_system: TVSystem,
    movie_player: Option<MoviePlayer>,
    movie_recorder: Option<MovieRecorder>,
    recorded_movie: Option<Movie>,
    /// NSF播放驱动（普通卡带为None）
    nsf: Option<Box<nsf::NsfDriver>>,
    /// NSF调用间隙挂起CPU（防止在哨兵地址执行垃圾代码）；
    /// 非返回式PLAY进行中为false（CPU持续执行曲程序）。
    nsf_halt_cpu: bool,
    #[cfg(feature = "debug")]
    breakpoints: Vec<Breakpoint>,
    #[cfg(feature = "debug")]
    cached_mem_breakpoints: Vec<Breakpoint>,
    #[cfg(feature = "debug")]
    paused: bool,
    #[cfg(feature = "debug")]
    breakpoint_hit: Option<Breakpoint>,
}

impl NES {
    pub fn new() -> Self {
        Self {
            cpu: cpu::CPU::new(),
            bus: bus::NESBus::new(),
            master_clock: 0,
            cpu_ppu_counter: 0,
            cpu_schedule_index: 0,
            cached_tv_system: TVSystem::NTSC,
            movie_player: None,
            movie_recorder: None,
            recorded_movie: None,
            nsf: None,
            nsf_halt_cpu: false,
            #[cfg(feature = "debug")]
            breakpoints: Vec::new(),
            #[cfg(feature = "debug")]
            cached_mem_breakpoints: Vec::new(),
            #[cfg(feature = "debug")]
            paused: false,
            #[cfg(feature = "debug")]
            breakpoint_hit: None,
        }
    }

    pub fn reset(&mut self) {
        self.bus.reset();
        self.reset_cpu_schedule();
        self.cpu.reset(&mut self.bus);
        self.cpu.set_nmi(self.bus.ppu_nmi_line());
        // NSF模式：RESET语义为当前曲目重头播放
        if let Some(driver) = &self.nsf {
            let track = driver.track;
            self.nsf_start_track(track);
        }
    }

    /// 模拟断电重启：CPU RAM 清零后复位。
    /// 播放从上电开始的 FM2 录像前调用，使机器回到确定初始状态
    /// （近似：VRAM 等未清零，游戏通常自行初始化，对绝大多数录像足够）。
    pub fn power_cycle(&mut self) {
        self.bus.ram = [0; 0x800];
        self.reset();
    }

    pub fn insert_cartridge(&mut self, cartridge: Cartridge) {
        self.clear_movie_state();
        self.nsf = None;
        self.nsf_halt_cpu = false;
        self.bus.insert_cartridge(cartridge);
        self.reset_cpu_schedule();
        self.reset();
    }

    pub fn load_cartridge_ines(&mut self, rom: &[u8]) -> Result<(), CartridgeError> {
        self.clear_movie_state();
        self.nsf = None;
        self.nsf_halt_cpu = false;
        self.bus.load_cartridge_ines(rom)?;
        self.reset_cpu_schedule();
        Ok(())
    }

    pub fn load_cartridge_fds(&mut self, image: &[u8], bios: &[u8]) -> Result<(), CartridgeError> {
        self.clear_movie_state();
        self.nsf = None;
        self.nsf_halt_cpu = false;
        self.bus.load_rom(image, Some(bios))?;
        self.reset_cpu_schedule();
        Ok(())
    }

    /// 按格式嗅探加载ROM（iNES/NSF/FDS镜像；FDS需要BIOS ROM数据）。
    pub fn load_rom(&mut self, rom: &[u8], bios: Option<&[u8]>) -> Result<(), CartridgeError> {
        self.clear_movie_state();
        if nsf::is_nsf_image(rom) {
            return self.load_nsf(rom);
        }
        self.nsf = None;
        self.nsf_halt_cpu = false;
        self.bus.load_rom(rom, bios)?;
        self.reset_cpu_schedule();
        Ok(())
    }

    /// 加载NSF并立即执行首曲INIT（曲号取头部起始曲）。
    fn load_nsf(&mut self, rom: &[u8]) -> Result<(), CartridgeError> {
        let header = nsf::parse_nsf(rom)?;
        let cartridge = Cartridge::from_nsf(&header)?;
        self.bus.insert_cartridge(cartridge);
        self.reset_cpu_schedule();
        self.nsf = None;
        self.nsf_halt_cpu = false;
        self.reset();
        let tv = header.tv_system();
        let cpu_hz = match tv {
            TVSystem::NTSC | TVSystem::DENDY => apu::CPU_CLOCK_NTSC,
            TVSystem::PAL => apu::CPU_CLOCK_PAL,
        };
        self.nsf = Some(Box::new(nsf::NsfDriver {
            info: header.info(header.start_song),
            init_addr: header.init_addr,
            play_addr: header.play_addr,
            track: header.start_song,
            play_period_cycles: nsf::speed_us_to_cycles(header.play_speed_us(tv), cpu_hz),
            play_acc: 0,
            last_service: self.bus.apu_cycle_count(),
            play_running: false,
        }));
        let start = header.start_song;
        self.nsf_start_track(start);
        Ok(())
    }

    /// 录像属于当前游戏，换卡时停止播放/录像并丢弃未保存数据。
    fn clear_movie_state(&mut self) {
        self.movie_player = None;
        self.movie_recorder = None;
        self.recorded_movie = None;
    }

    /// 当前卡带 PRG‖CHR 的 MD5 十六进制字符串（fm2 `romChecksum` 口径）。
    pub fn rom_md5_hex(&self) -> Option<String> {
        self.bus.rom_md5_hex()
    }

    /// 当前卡带 PRG‖CHR 的 MD5 原始字节。
    pub fn rom_md5(&self) -> Option<[u8; 16]> {
        self.bus.rom_md5()
    }

    /// 当前卡带 PRG‖CHR 的 CRC-32（Nestopia .cht `game` 属性口径）。
    pub fn rom_crc32(&self) -> Option<u32> {
        self.bus.rom_crc32()
    }

    pub fn fds_command(&mut self, cmd: FdsDiskCommand) {
        self.bus.fds_command(cmd);
    }

    pub fn fds_info(&self) -> Option<FdsDiskInfo> {
        self.bus.fds_info()
    }

    /// FDS磁盘是否被写入过；非FDS卡带返回false。
    pub fn fds_dirty(&self) -> bool {
        self.bus.fds_dirty()
    }

    /// FDS每面磁盘数据（写回.sav用）；非FDS卡带返回None。
    pub fn fds_sides(&self) -> Option<&[Vec<u8>]> {
        self.bus.fds_sides()
    }

    /// 电池后备PRG RAM（读写.sav用）；无电池卡带返回None。
    pub fn battery_ram(&self) -> Option<&[u8]> {
        self.bus.battery_ram()
    }

    /// 载入电池后备PRG RAM（无电池卡带为空操作）。
    pub fn load_battery_ram(&mut self, data: &[u8]) {
        self.bus.load_battery_ram(data)
    }

    pub fn set_controller_state(&mut self, port: usize, state: ControllerState) {
        self.bus.set_controller_state(port, state);
    }

    /// 当前手柄状态;端口越界返回全释放状态。
    pub fn controller_state(&self, port: usize) -> ControllerState {
        self.bus.controller_state(port)
    }

    /// 金手指主列表（含未启用项，按添加顺序）。
    pub fn cheats(&self) -> &[Cheat] {
        self.bus.cheats()
    }

    /// 整体替换金手指列表（加载 .cht 文件用）。
    pub fn set_cheats(&mut self, cheats: Vec<Cheat>) {
        self.bus.set_cheats(cheats);
    }

    pub fn add_cheat(&mut self, cheat: Cheat) {
        self.bus.add_cheat(cheat);
    }

    /// 按下标移除金手指；返回是否存在并被移除。
    pub fn remove_cheat(&mut self, index: usize) -> bool {
        self.bus.remove_cheat(index)
    }

    /// 启/禁用某条金手指；返回是否存在并被修改。
    pub fn set_cheat_enabled(&mut self, index: usize, enabled: bool) -> bool {
        self.bus.set_cheat_enabled(index, enabled)
    }

    pub fn clear_cheats(&mut self) {
        self.bus.clear_cheats();
    }

    pub fn clock(&mut self) {
        #[cfg(feature = "debug")]
        if self.paused() {
            return;
        }

        self.master_clock += 1;
        self.bus.tick_ppu();

        #[cfg(feature = "debug")]
        {
            if self.check_ppu_breakpoints() {
                return;
            }
        }

        let threshold = match self.cached_tv_system {
            TVSystem::NTSC | TVSystem::DENDY => (3, 1),
            TVSystem::PAL => (PAL_CPU_SCHEDULE[self.cpu_schedule_index], 5),
        };
        self.cpu_ppu_counter += 1;
        if self.cpu_ppu_counter >= threshold.0 {
            self.cpu_ppu_counter = 0;
            self.cpu_schedule_index = (self.cpu_schedule_index + 1) % threshold.1;
            // Update NMI/IRQ only around CPU step, not every PPU cycle
            self.cpu.set_nmi(self.bus.ppu_nmi_line());
            self.bus.advance_dma_cpu_phase();
            self.bus.tick_apu_cpu_cycle();

            #[cfg(feature = "debug")]
            {
                self.bus
                    .set_debug_mem_breakpoints(&self.cached_mem_breakpoints);
            }

            if !self.nsf_halt_cpu {
                self.cpu.clock(&mut self.bus);
            }

            #[cfg(feature = "debug")]
            {
                if let Some(bp) = self.bus.take_mem_breakpoint_hit() {
                    self.breakpoint_hit = Some(bp);
                    return;
                }
            }

            self.cpu.irq_set_level(0x01, self.bus.apu_irq_line());
            self.cpu.irq_set_level(0x02, self.bus.cartridge_irq_line());
            self.cpu.set_nmi(self.bus.ppu_nmi_line());

            #[cfg(feature = "debug")]
            {
                if self.check_breakpoints() {}
            }
        }
    }

    pub fn run_frame(&mut self) {
        if self.nsf.is_some() {
            self.run_nsf_frame();
            return;
        }
        let start_frame = self.frame_number();
        while self.frame_number() == start_frame {
            self.clock();
        }
    }

    // ===== NSF 播放 =====

    /// NSF元数据；非NSF模式返回None。
    pub fn nsf_info(&self) -> Option<&nsf::NsfInfo> {
        self.nsf.as_ref().map(|driver| &driver.info)
    }

    /// 当前NSF曲目（1起）；非NSF模式返回None。
    pub fn nsf_track(&self) -> Option<u16> {
        self.nsf.as_ref().map(|driver| driver.track)
    }

    /// 切换NSF曲目（1起，越界自动钳制）并重新INIT。
    /// 非NSF模式返回false。
    pub fn nsf_select_track(&mut self, track: u16) -> bool {
        let Some(driver) = self.nsf.as_ref() else {
            return false;
        };
        let track = track.clamp(1, driver.info.track_count);
        self.nsf_start_track(track);
        true
    }

    /// NSF切曲：规范初始序列（清RAM/APU/芯片、恢复bank）后调用INIT。
    fn nsf_start_track(&mut self, track: u16) {
        let (fds, pal, init_addr) = {
            let driver = self.nsf.as_ref().expect("nsf_start_track without driver");
            (
                driver.info.chips & 0x04 != 0,
                matches!(driver.info.tv_system, TVSystem::PAL),
                driver.init_addr,
            )
        };
        {
            let driver = self.nsf.as_mut().unwrap();
            driver.track = track;
            driver.info.current_track = track;
            driver.play_acc = 0;
            driver.play_running = false;
        }
        self.bus.clear_apu_audio_samples();
        self.bus.nsf_prepare_song(fds);
        if init_addr != 0 {
            self.nsf_halt_cpu = false;
            self.cpu.nsf_call_init(
                &mut self.bus,
                init_addr,
                (track - 1) as u8,
                pal,
                nsf::NSF_SENTINEL_ADDR,
            );
            // INIT预算耗尽（非返回式INIT）极罕见：交由PLAY接管
            let returned = self.nsf_run_until_sentinel(NSF_INIT_BUDGET_CYCLES);
            self.nsf_halt_cpu = returned;
        } else {
            self.nsf_halt_cpu = true;
        }
        let driver = self.nsf.as_mut().unwrap();
        driver.last_service = self.bus.apu_cycle_count();
    }

    /// NSF一帧：跑完一个PPU帧后按微秒速度字结算PLAY调用。
    fn run_nsf_frame(&mut self) {
        let start_frame = self.frame_number();
        while self.frame_number() == start_frame {
            self.clock();
        }
        let (period, play_addr) = {
            let driver = self.nsf.as_mut().expect("run_nsf_frame without driver");
            let now = self.bus.apu_cycle_count();
            driver.play_acc += (now - driver.last_service) as i64;
            driver.last_service = now;
            (driver.play_period_cycles as i64, driver.play_addr)
        };
        let budget = self.nsf.as_ref().unwrap().play_period_cycles * 3 + 20_000;
        // 非返回式PLAY仍在执行：不重入，吸收多余计时
        if self.nsf.as_ref().unwrap().play_running {
            let driver = self.nsf.as_mut().unwrap();
            driver.play_acc = driver.play_acc.min(period - 1);
            return;
        }
        if play_addr == 0 {
            return;
        }
        loop {
            let due = {
                let driver = self.nsf.as_ref().unwrap();
                driver.play_acc >= period && !driver.play_running
            };
            if !due {
                break;
            }
            {
                let driver = self.nsf.as_mut().unwrap();
                driver.play_acc -= period;
            }
            self.nsf_halt_cpu = false;
            self.cpu
                .nsf_call_play(&mut self.bus, play_addr, nsf::NSF_SENTINEL_ADDR);
            let returned = self.nsf_run_until_sentinel(budget);
            self.nsf_halt_cpu = returned;
            {
                let driver = self.nsf.as_mut().unwrap();
                driver.play_running = !returned;
                // 调用耗时从自由运行计时扣除（近似硬件持续计时）
                let now = self.bus.apu_cycle_count();
                let spent = (now - driver.last_service) as i64;
                driver.play_acc -= spent.min(driver.play_acc);
                driver.last_service = now;
            }
        }
    }

    /// 执行NSF例程直到PC回到哨兵（RTS返回）或预算耗尽。
    /// 返回true=正常返回；false=预算耗尽（非返回式PLAY或损坏文件）。
    fn nsf_run_until_sentinel(&mut self, budget_cycles: u64) -> bool {
        let start = self.bus.apu_cycle_count();
        loop {
            let before_instruction = self.cpu.instruction_counter();
            let before_master = self.master_clock;
            while self.cpu.instruction_counter() == before_instruction {
                self.clock();
                if self.master_clock == before_master {
                    // 调试暂停/断点短路了时钟：按已返回处理避免死循环
                    return true;
                }
                if self.bus.apu_cycle_count().saturating_sub(start) > budget_cycles {
                    return false;
                }
            }
            if self.cpu.pc() == nsf::NSF_SENTINEL_ADDR {
                return true;
            }
        }
    }

    /// [`CoreCommand::RunFrame`] 的电影感知版本：
    /// 播放中先套用录像输入（含 Reset 命令），录像中先快照当前手柄状态。
    /// 播完最后一帧返回 [`CoreEvent::MovieFinished`]。
    fn run_movie_aware_frame(&mut self) -> CoreEvent {
        // 录像机：帧前快照（Copy 数据，借用在语句内结束）。
        if let Some(recorder) = self.movie_recorder.as_mut() {
            let frame = MovieFrame {
                commands: u32::from(recorder.pending_reset),
                controllers: [self.bus.controller_state(0), self.bus.controller_state(1)],
            };
            recorder.pending_reset = false;
            recorder.movie.frames.push(frame);
        }

        // 播放器：取当前帧（Copy），避免跨方法借用。
        let current = match &self.movie_player {
            Some(player) if !player.finished => Some(player.movie.frames[player.frame]),
            _ => None,
        };
        let mut movie_finished = false;
        if let Some(frame) = current {
            if frame.reset() {
                self.movie_reset();
            }
            self.bus.set_controller_state(0, frame.controllers[0]);
            self.bus.set_controller_state(1, frame.controllers[1]);
            self.run_frame();
            let player = self.movie_player.as_mut().expect("player just checked");
            player.frame += 1;
            if player.frame >= player.movie.frames.len() {
                player.finished = true;
                movie_finished = true;
            }
        } else {
            self.run_frame();
        }

        if movie_finished {
            CoreEvent::MovieFinished
        } else {
            CoreEvent::FrameReady {
                frame_number: self.frame_number(),
            }
        }
    }

    /// 近似硬件 RESET 键：仅复位 CPU（读取复位向量），PPU/APU 状态保留。
    fn movie_reset(&mut self) {
        self.cpu.reset(&mut self.bus);
        self.cpu.set_nmi(self.bus.ppu_nmi_line());
    }

    /// 正在播放的电影（含进度信息）；未在播放返回 None。
    pub fn movie(&self) -> Option<&Movie> {
        self.movie_player.as_ref().map(|p| &p.movie)
    }

    /// 播放进度：下一帧下标（等于帧数时已播完）。
    pub fn movie_frame(&self) -> Option<usize> {
        self.movie_player.as_ref().map(|p| p.frame)
    }

    /// 是否正在录像。
    pub fn is_recording_movie(&self) -> bool {
        self.movie_recorder.is_some()
    }

    /// 取走已完成的录像（StopMovieRecording 后可取，仅一次）。
    pub fn take_recorded_movie(&mut self) -> Option<Movie> {
        self.recorded_movie.take()
    }

    pub fn step_cpu_instruction(&mut self) {
        let start_instruction = self.cpu.instruction_counter();
        while self.cpu.instruction_counter() == start_instruction {
            self.clock();
        }
    }

    pub fn execute(&mut self, command: CoreCommand) -> CoreResponse {
        let event = match command {
            CoreCommand::Reset => {
                if let Some(recorder) = &mut self.movie_recorder {
                    recorder.pending_reset = true;
                }
                self.reset();
                CoreEvent::ResetComplete
            }
            CoreCommand::SetControllerState { port, state } => {
                self.set_controller_state(port, state);
                CoreEvent::ControllerStateUpdated { port }
            }
            CoreCommand::RunFrame => self.run_movie_aware_frame(),
            CoreCommand::StepCpuInstruction => {
                self.step_cpu_instruction();
                CoreEvent::CpuInstructionComplete {
                    instruction_counter: self.cpu.instruction_counter(),
                }
            }
            #[cfg(feature = "debug")]
            CoreCommand::AddBreakpoint(bp) => {
                self.add_breakpoint(bp);
                CoreEvent::None
            }
            #[cfg(feature = "debug")]
            CoreCommand::RemoveBreakpoint(bp) => {
                self.remove_breakpoint(&bp);
                CoreEvent::None
            }
            #[cfg(feature = "debug")]
            CoreCommand::SetPaused(paused) => {
                self.set_paused(paused);
                CoreEvent::None
            }
            CoreCommand::FdsDisk(cmd) => {
                self.fds_command(cmd);
                CoreEvent::FdsDiskUpdated
            }
            CoreCommand::NsfSelectTrack(track) => {
                self.nsf_select_track(track);
                CoreEvent::None
            }
            CoreCommand::SetCheats(cheats) => {
                self.set_cheats(cheats);
                CoreEvent::CheatsUpdated
            }
            CoreCommand::AddCheat(cheat) => {
                self.add_cheat(cheat);
                CoreEvent::CheatsUpdated
            }
            CoreCommand::RemoveCheat(index) => {
                self.remove_cheat(index);
                CoreEvent::CheatsUpdated
            }
            CoreCommand::SetCheatEnabled { index, enabled } => {
                self.set_cheat_enabled(index, enabled);
                CoreEvent::CheatsUpdated
            }
            CoreCommand::ClearCheats => {
                self.clear_cheats();
                CoreEvent::CheatsUpdated
            }
            CoreCommand::PlayMovie(movie) => {
                self.movie_recorder = None;
                self.recorded_movie = None;
                let finished = movie.frames.is_empty();
                self.movie_player = Some(MoviePlayer {
                    movie,
                    frame: 0,
                    finished,
                });
                if finished {
                    CoreEvent::MovieFinished
                } else {
                    CoreEvent::None
                }
            }
            CoreCommand::StopMovie => {
                self.movie_player = None;
                CoreEvent::None
            }
            CoreCommand::StartMovieRecording => {
                self.movie_player = None;
                self.recorded_movie = None;
                self.movie_recorder = Some(MovieRecorder {
                    movie: Movie {
                        version: 3,
                        ports: [
                            crate::movie::port_kind::GAMEPAD,
                            crate::movie::port_kind::GAMEPAD,
                            crate::movie::port_kind::NONE,
                        ],
                        // 自动填元数据：ROM 校验和/制式/模拟器版本
                        rom_checksum: self.bus.rom_md5_hex().unwrap_or_default(),
                        pal_flag: matches!(
                            self.bus.ppu().tv_system(),
                            TVSystem::PAL | TVSystem::DENDY
                        ),
                        emu_version: emulator_version_number(),
                        ..Movie::default()
                    },
                    pending_reset: false,
                });
                CoreEvent::None
            }
            CoreCommand::StopMovieRecording => {
                if let Some(recorder) = self.movie_recorder.take() {
                    self.recorded_movie = Some(recorder.movie);
                }
                CoreEvent::None
            }
        };

        CoreResponse {
            event,
            master_clock: self.master_clock,
        }
    }

    pub fn master_clock(&self) -> u64 {
        self.master_clock
    }

    /// 当前制式（由卡带头部解析，或经 [`NES::set_tv_system`] 覆盖）。
    pub fn tv_system(&self) -> TVSystem {
        self.bus.ppu().tv_system()
    }

    /// 覆盖当前制式（重新计时为 NTSC/PAL/DENDY）。
    /// 影响PPU帧长、CPU/PPU分频调度与APU帧序列器/周期表/采样率主频。
    pub fn set_tv_system(&mut self, tv: TVSystem) {
        self.bus.set_tv_system(tv);
        self.cached_tv_system = tv;
        // PAL 的 CPU/PPU 分频调度相位按新制式重新对齐
        self.cpu_schedule_index = 0;
        self.cpu_ppu_counter = 0;
    }

    pub fn frame_number(&self) -> u64 {
        self.bus.ppu_frame()
    }

    pub fn frame_pixels(&self) -> &[u8] {
        self.bus.ppu().frame_pixels()
    }

    pub fn video_frame(&self) -> VideoFrame<'_> {
        VideoFrame {
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            pitch: VIDEO_FRAME_PITCH,
            format: PixelFormat::Indexed8,
            frame_number: self.frame_number(),
            pixels: self.frame_pixels(),
            emphasis: self.emphasis_rgb_bits(),
        }
    }

    /// $2001 强调位换算为 RGB 语义掩码（bit0=R, bit1=G, bit2=B）。
    /// PAL/Dendy 上 bit5/6 的含义与 NTSC 互换（Nesdev Colour emphasis）。
    fn emphasis_rgb_bits(&self) -> u8 {
        let bits = self.bus.ppu().emphasis_bits();
        let (r, g, b) = match self.cached_tv_system {
            TVSystem::NTSC => (bits & 1, (bits >> 1) & 1, (bits >> 2) & 1),
            TVSystem::PAL | TVSystem::DENDY => ((bits >> 1) & 1, bits & 1, (bits >> 2) & 1),
        };
        r | (g << 1) | (b << 2)
    }

    pub fn audio_batch(&self) -> AudioBatch<'_> {
        AudioBatch {
            channels: 1,
            sample_rate: self.bus.apu_sample_rate(),
            samples: self.bus.apu_audio_samples(),
        }
    }

    pub fn apu_audio_samples(&self) -> &[f32] {
        self.bus.apu_audio_samples()
    }

    pub fn apu_sample_rate(&self) -> u32 {
        self.bus.apu_sample_rate()
    }

    pub fn clear_apu_audio_samples(&mut self) {
        self.bus.clear_apu_audio_samples();
    }

    pub fn add_expansion_audio_chip(&mut self, chip: Box<dyn ExpansionAudioChip>) {
        self.bus.add_expansion_audio_chip(chip);
    }

    pub fn debug_snapshot(&self) -> DebugSnapshot {
        let ppu = self.bus.ppu();
        DebugSnapshot {
            master_clock: self.master_clock,
            cpu: self.cpu.debug_snapshot(),
            #[cfg(feature = "debug")]
            ppu: PpuDebugSnapshot {
                frame: ppu.frame(),
                scanline: ppu.scanline(),
                in_vblank: ppu.in_vblank(),
                nmi_line: ppu.nmi_line(),
                oam_addr: ppu.oam_addr(),
                cycles: ppu.cycles(),
                ctrl: ppu.debug_ctrl(),
                mask: ppu.debug_mask(),
                status: ppu.debug_status(),
                fine_x: ppu.debug_fine_x(),
                vram_addr: ppu.debug_vram_addr(),
                temp_vram_addr: ppu.debug_temp_vram_addr(),
                write_latch: ppu.debug_write_latch(),
                bg_on: ppu.bg_on(),
                sprites_on: ppu.sprites_on(),
                rendering_on: ppu.rendering_on(),
                odd_frame: ppu.debug_odd_frame(),
            },
            #[cfg(not(feature = "debug"))]
            ppu: PpuDebugSnapshot {
                frame: ppu.frame(),
                scanline: ppu.scanline(),
                in_vblank: ppu.in_vblank(),
                nmi_line: ppu.nmi_line(),
                oam_addr: ppu.oam_addr(),
            },
        }
    }

    #[cfg(feature = "debug")]
    pub fn debug_memory_snapshot(&self) -> MemorySnapshot<'_> {
        self.bus.debug_memory_snapshot()
    }

    pub fn mirroring(&self) -> Mirroring {
        self.bus.mirroring()
    }

    #[cfg(feature = "debug")]
    pub fn debug_read_chr(&mut self) -> [u8; 0x2000] {
        self.bus.debug_read_chr()
    }

    #[cfg(feature = "debug")]
    pub fn debug_disassemble(&mut self, rows: usize) -> DisassemblyResult {
        let pc = self.cpu.pc();
        cpu::disassemble_range(&mut self.bus, pc, rows, rows)
    }

    /// 任意CPU总线地址读取（调试器用）。
    #[cfg(feature = "debug")]
    pub fn debug_cpu_read(&mut self, addr: u16) -> u8 {
        crate::bus::CPUBus::cpu_read(&mut self.bus, addr)
    }

    pub fn save_state(&self) -> Result<Vec<u8>, SaveStateError> {
        let mut writer = StateWriter::new();
        writer.write_u64(self.master_clock);
        writer.write_u8(self.cpu_ppu_counter);
        writer.write_u64(self.cpu_schedule_index as u64);
        self.cpu.save_state(&mut writer);
        self.bus.save_state(&mut writer)?;
        // NSF驱动计时块（旧版存档没有这段，读取侧按剩余字节探测）
        if let Some(driver) = &self.nsf {
            writer.write_bool(true);
            writer.write_u16(driver.track);
            writer.write_u64(driver.play_acc as u64);
            writer.write_u64(driver.last_service);
            writer.write_bool(driver.play_running);
        } else {
            writer.write_bool(false);
        }
        Ok(writer.finish())
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), SaveStateError> {
        let mut reader = StateReader::new(bytes)?;
        self.master_clock = reader.read_u64()?;
        self.cpu_ppu_counter = reader.read_u8()?;
        self.cpu_schedule_index = reader.read_u64()? as usize;
        self.cpu.load_state(&mut reader)?;
        self.bus.load_state(&mut reader)?;
        self.cached_tv_system = self.bus.ppu().tv_system();
        self.cpu.set_nmi(self.bus.ppu_nmi_line());
        // 追加的NSF块：旧版存档（非NSF机器写入）没有这部分
        if !reader.is_empty() {
            if reader.read_bool()? {
                let track = reader.read_u16()?;
                let play_acc = reader.read_u64()? as i64;
                let last_service = reader.read_u64()?;
                let play_running = reader.read_bool()?;
                if let Some(driver) = self.nsf.as_mut() {
                    driver.track = track;
                    driver.info.current_track = track;
                    driver.play_acc = play_acc;
                    driver.last_service = last_service;
                    driver.play_running = play_running;
                }
                // 调用间隙挂起；非返回式PLAY进行中则持续执行
                self.nsf_halt_cpu = !play_running;
            } else {
                self.nsf = None;
                self.nsf_halt_cpu = false;
            }
        }
        reader.finish()
    }

    pub fn clear_audio_samples(&mut self) {
        self.bus.clear_apu_audio_samples();
    }

    pub fn set_apu_sample_rate(&mut self, sample_rate: u32) {
        self.bus.set_apu_sample_rate(sample_rate);
    }

    pub fn set_apu_debug_mute_mask(&mut self, mask: u8) {
        self.bus.set_apu_debug_mute_mask(mask);
    }

    pub fn apu_debug_mute_mask(&self) -> u8 {
        self.bus.apu_debug_mute_mask()
    }

    #[cfg(feature = "debug")]
    fn update_mem_breakpoint_cache(&mut self) {
        self.cached_mem_breakpoints = self
            .breakpoints
            .iter()
            .copied()
            .filter(|bp| matches!(bp, Breakpoint::MemoryRead(_) | Breakpoint::MemoryWrite(_)))
            .collect();
    }

    #[cfg(feature = "debug")]
    pub fn add_breakpoint(&mut self, bp: Breakpoint) {
        if !self.breakpoints.contains(&bp) {
            self.breakpoints.push(bp);
            self.update_mem_breakpoint_cache();
        }
    }

    #[cfg(feature = "debug")]
    pub fn remove_breakpoint(&mut self, bp: &Breakpoint) {
        self.breakpoints.retain(|b| b != bp);
        self.update_mem_breakpoint_cache();
    }

    #[cfg(feature = "debug")]
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
        self.cached_mem_breakpoints.clear();
    }

    #[cfg(feature = "debug")]
    pub fn breakpoints(&self) -> &[Breakpoint] {
        &self.breakpoints
    }

    #[cfg(feature = "debug")]
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.breakpoint_hit = None;
    }

    #[cfg(feature = "debug")]
    pub fn paused(&self) -> bool {
        self.paused || self.breakpoint_hit.is_some()
    }

    #[cfg(feature = "debug")]
    pub fn breakpoint_hit(&self) -> Option<Breakpoint> {
        self.breakpoint_hit
    }

    #[cfg(feature = "debug")]
    fn check_breakpoints(&mut self) -> bool {
        if self.breakpoints.is_empty() {
            return false;
        }

        let pc = self.cpu.pc();

        for &bp in &self.breakpoints {
            match bp {
                Breakpoint::Address(addr) if pc == addr => {
                    self.breakpoint_hit = Some(bp);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    #[cfg(feature = "debug")]
    fn check_ppu_breakpoints(&mut self) -> bool {
        if self.breakpoints.is_empty() {
            return false;
        }

        let ppu = self.bus.ppu();
        let scanline = ppu.scanline();
        let in_vblank = ppu.in_vblank();

        for &bp in &self.breakpoints {
            match bp {
                Breakpoint::PpuScanline(sl) if scanline == sl => {
                    self.breakpoint_hit = Some(bp);
                    return true;
                }
                Breakpoint::Vblank if in_vblank => {
                    self.breakpoint_hit = Some(bp);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn reset_cpu_schedule(&mut self) {
        self.cached_tv_system = self.bus.ppu().tv_system();
        self.cpu_ppu_counter = 0;
        self.cpu_schedule_index = 0;
    }
}

/// 模拟器版本号数字化（"0.3.0" → 300），写入录像 emuVersion。
fn emulator_version_number() -> u32 {
    let mut parts = env!("CARGO_PKG_VERSION").split('.');
    let mut parse = || parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    parse() * 10_000 + parse() * 100 + parse()
}

impl Default for NES {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
