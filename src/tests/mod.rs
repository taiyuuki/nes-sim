use super::{
    ControllerButton, ControllerState, CoreCommand, CoreEvent, FrontendInput, FrontendRuntime, NES,
    RunMode,
};
use crate::bus::CPUBus;
use crate::headless::{frame_to_ppm, stable_byte_hash, write_frame_ppm};
use std::io;

#[derive(Debug, PartialEq, Eq)]
struct NestestTraceEntry {
    pc: u16,
    a: u8,
    x: u8,
    y: u8,
    p: u8,
    sp: u8,
    cyc: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AccuracyCoinResult {
    raw: u8,
    status: u8,
    error_code: u8,
}

fn parse_hex_u16(text: &str) -> u16 {
    u16::from_str_radix(text, 16).expect("hex u16 field should parse")
}

fn parse_hex_u8(text: &str) -> u8 {
    u8::from_str_radix(text, 16).expect("hex u8 field should parse")
}

fn parse_dec_u64(text: &str) -> u64 {
    text.trim()
        .parse::<u64>()
        .expect("decimal field should parse")
}

fn parse_nestest_trace_line(line: &str) -> NestestTraceEntry {
    let pc = parse_hex_u16(&line[..4]);
    let a_index = line.find("A:").expect("trace line should contain A:");
    let x_index = line.find("X:").expect("trace line should contain X:");
    let y_index = line.find("Y:").expect("trace line should contain Y:");
    let p_index = line.find("P:").expect("trace line should contain P:");
    let sp_index = line.find("SP:").expect("trace line should contain SP:");
    let cyc_index = line.find("CYC:").expect("trace line should contain CYC:");

    NestestTraceEntry {
        pc,
        a: parse_hex_u8(&line[a_index + 2..a_index + 4]),
        x: parse_hex_u8(&line[x_index + 2..x_index + 4]),
        y: parse_hex_u8(&line[y_index + 2..y_index + 4]),
        p: parse_hex_u8(&line[p_index + 2..p_index + 4]),
        sp: parse_hex_u8(&line[sp_index + 3..sp_index + 5]),
        cyc: parse_dec_u64(&line[cyc_index + 4..]),
    }
}

fn set_controller_buttons(nes: &mut NES, buttons: &[ControllerButton]) {
    let mut state = ControllerState::new();
    for &button in buttons {
        state.set_pressed(button, true);
    }
    nes.set_controller_state(0, state);
}

fn tap_controller_buttons(nes: &mut NES, buttons: &[ControllerButton]) {
    set_controller_buttons(nes, buttons);
    nes.run_frame();
    nes.run_frame();
    nes.set_controller_state(0, ControllerState::new());
    nes.run_frame();
    nes.run_frame();
}

fn parse_accuracy_coin_result(raw: u8) -> AccuracyCoinResult {
    AccuracyCoinResult {
        raw,
        status: raw & 0x03,
        error_code: raw >> 2,
    }
}

fn read_optional_binary_fixture(path: &str) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            eprintln!("skipping fixture-dependent test because {path} is missing");
            None
        }
        Err(error) => panic!("failed to read {path}: {error}"),
    }
}

fn read_optional_text_fixture(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            eprintln!("skipping fixture-dependent test because {path} is missing");
            None
        }
        Err(error) => panic!("failed to read {path}: {error}"),
    }
}

fn run_accuracy_coin_page(
    page_index: usize,
    result_addrs: &[u16],
) -> Option<(NES, Vec<AccuracyCoinResult>)> {
    let rom = read_optional_binary_fixture("roms/AccuracyCoin/AccuracyCoin.nes")?;
    let mut nes = NES::new();

    nes.load_cartridge_ines(&rom)
        .expect("AccuracyCoin ROM should load as NROM");
    nes.reset();

    for _ in 0..120 {
        nes.run_frame();
    }

    for _ in 0..page_index {
        tap_controller_buttons(&mut nes, &[ControllerButton::Right]);
        for _ in 0..10 {
            nes.run_frame();
        }
    }

    tap_controller_buttons(&mut nes, &[ControllerButton::A]);

    let max_frames = 2_000usize;
    for _ in 0..max_frames {
        let done = result_addrs.iter().all(|&addr| {
            matches!(
                parse_accuracy_coin_result(nes.bus.ram[addr as usize]).status,
                1 | 2
            )
        });
        if done {
            let results = result_addrs
                .iter()
                .map(|&addr| parse_accuracy_coin_result(nes.bus.ram[addr as usize]))
                .collect();
            return Some((nes, results));
        }
        nes.run_frame();
    }

    let results = result_addrs
        .iter()
        .map(|&addr| parse_accuracy_coin_result(nes.bus.ram[addr as usize]))
        .collect();
    Some((nes, results))
}

fn boot_rom(path: &str, frames: usize) -> Option<NES> {
    let rom = read_optional_binary_fixture(path)?;
    let mut nes = NES::new();

    nes.load_cartridge_ines(&rom)
        .expect("ROM should load as a supported mapper");
    nes.reset();

    for _ in 0..frames {
        nes.run_frame();
    }

    Some(nes)
}

#[test]
fn smb1_hud_coin_does_not_leak_hidden_black_helper_pixel() {
    let Some(mut nes) = boot_rom("roms/nrom/Super Mario Bros. (W) [!].nes", 120) else {
        return;
    };

    tap_controller_buttons(&mut nes, &[ControllerButton::Start]);
    for _ in 0..240 {
        nes.run_frame();
    }

    let frame = nes.frame_pixels();
    assert_eq!(
        frame[30 * 256 + 89],
        0x22,
        "the sky pixel immediately left of the HUD coin should stay blue instead of leaking sprite 0's black guide pixel"
    );
    assert_eq!(
        frame[30 * 256 + 91],
        0x17,
        "the coin body should still render after the left-adjacent sky pixel"
    );
    assert_eq!(
        frame[30 * 256 + 90],
        0x17,
        "the lower-left coin edge should remain part of the coin instead of turning into a leaked black helper pixel"
    );
}

fn visible_frame_has_non_background_content(nes: &NES) -> bool {
    nes.frame_pixels().iter().any(|&pixel| pixel != 0)
}

fn assert_rom_boot_frame_hash(
    rom_path: &str,
    frames: usize,
    expected_hash: u64,
    failure_ppm_path: &str,
) {
    let Some(nes) = boot_rom(rom_path, frames) else {
        return;
    };

    assert!(
        visible_frame_has_non_background_content(&nes),
        "expected {rom_path} boot sequence to render visible non-zero palette indices by frame {}",
        nes.frame_number()
    );

    let ppm = frame_to_ppm(nes.video_frame());
    let actual_hash = stable_byte_hash(&ppm);
    if actual_hash != expected_hash {
        let failure_path = std::path::Path::new(failure_ppm_path);
        if let Some(parent) = failure_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("failed to create {:?}: {}", parent, error));
        }
        write_frame_ppm(failure_path, nes.video_frame())
            .unwrap_or_else(|error| panic!("failed to write {:?}: {}", failure_path, error));
    }

    assert_eq!(
        actual_hash, expected_hash,
        "boot frame hash mismatch for {rom_path}; wrote actual frame to {failure_ppm_path}"
    );
}

fn make_ines_with_tv(flags9: u8) -> Vec<u8> {
    let mut rom = vec![0; 16];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    rom[9] = flags9;
    rom.extend(std::iter::repeat_n(0, 0x4000));
    rom.extend(std::iter::repeat_n(0, 0x2000));
    rom
}

fn make_ines_with_reset_vector(entry_point: u16) -> Vec<u8> {
    let mut rom = make_ines_with_tv(0x00);
    let prg_start = 16;
    rom[prg_start] = 0xEA;
    rom[prg_start + 1] = 0xEA;
    rom[prg_start + 0x3FFC] = entry_point as u8;
    rom[prg_start + 0x3FFD] = (entry_point >> 8) as u8;
    rom
}

#[test]
fn run_frame_advances_exactly_one_ppu_frame() {
    let mut nes = NES::new();
    let start_frame = nes.frame_number();

    nes.run_frame();

    assert_eq!(nes.frame_number(), start_frame + 1);
}

#[test]
fn pal_cpu_schedule_uses_33334_pattern() {
    let mut nes = NES::new();
    let rom = make_ines_with_tv(0x01);

    nes.load_cartridge_ines(&rom)
        .expect("PAL cartridge should load");

    for step in 1..=16 {
        nes.clock();
        let expected = match step {
            1..=2 => 0,
            3..=5 => 1,
            6..=8 => 2,
            9..=11 => 3,
            12..=15 => 4,
            16 => 5,
            _ => unreachable!(),
        };
        assert_eq!(nes.cpu.clocks(), expected, "master clock step {}", step);
    }
}

#[test]
fn tv_system_override_can_force_ntsc_cpu_schedule_for_pal_header() {
    let mut nes = NES::new();
    let rom = make_ines_with_tv(0x01);

    nes.load_cartridge_ines(&rom)
        .expect("cartridge should load with NTSC override");

    for step in 1..=6 {
        nes.clock();
        let expected = match step {
            1..=2 => 0,
            3..=5 => 1,
            6 => 2,
            _ => unreachable!(),
        };
        assert_eq!(nes.cpu.clocks(), expected, "master clock step {}", step);
    }
}

#[test]
fn nestest_rom_resets_to_c004_entry_point() {
    let Some(rom) = read_optional_binary_fixture("roms/nestest/nestest.nes") else {
        return;
    };
    let mut nes = NES::new();

    nes.load_cartridge_ines(&rom)
        .expect("nestest ROM should load as NROM");
    nes.reset();

    assert_eq!(nes.cpu.pc(), 0xC004);
}

#[test]
fn nes_exposes_controller_state_updates_through_the_bus() {
    let mut nes = NES::new();
    let mut state = ControllerState::new();
    state.set_pressed(ControllerButton::Start, true);
    nes.set_controller_state(0, state);

    nes.bus.cpu_write(0x4016, 0x01);
    nes.bus.cpu_write(0x4016, 0x00);

    assert_eq!(nes.bus.cpu_read(0x4016), 0);
    assert_eq!(nes.bus.cpu_read(0x4016), 0);
    assert_eq!(nes.bus.cpu_read(0x4016), 0);
    assert_eq!(nes.bus.cpu_read(0x4016), 1);
}

#[test]
fn frame_ppm_uses_binary_ppm_header_and_rgb_payload() {
    let nes = NES::new();

    assert_eq!(
        nes.frame_pixels().len(),
        crate::FRAME_WIDTH * crate::FRAME_HEIGHT
    );

    let ppm = frame_to_ppm(nes.video_frame());
    let header = b"P6\n256 240\n255\n";

    assert!(ppm.starts_with(header));
    assert_eq!(
        ppm.len(),
        header.len() + crate::FRAME_WIDTH * crate::FRAME_HEIGHT * 3
    );
    assert_eq!(
        &ppm[header.len()..header.len() + 3],
        &[84, 84, 84],
        "palette index 0 should map to the universal background color RGB triplet"
    );
}

#[test]
fn execute_run_frame_reports_frame_ready_event() {
    let mut nes = NES::new();
    let start_frame = nes.frame_number();

    let response = nes.execute(CoreCommand::RunFrame);

    assert_eq!(
        response.event,
        CoreEvent::FrameReady {
            frame_number: start_frame + 1,
        }
    );
    assert_eq!(nes.frame_number(), start_frame + 1);
    assert_eq!(response.master_clock, nes.master_clock());
}

#[test]
fn step_cpu_instruction_updates_debug_snapshot() {
    let mut nes = NES::new();
    let rom = make_ines_with_reset_vector(0x8000);
    nes.load_cartridge_ines(&rom)
        .expect("test ROM should load as NROM");
    nes.reset();

    let before = nes.debug_snapshot();
    let response = nes.execute(CoreCommand::StepCpuInstruction);
    let after = nes.debug_snapshot();

    assert_eq!(before.cpu.pc, 0x8000);
    assert_eq!(
        response.event,
        CoreEvent::CpuInstructionComplete {
            instruction_counter: 1,
        }
    );
    assert_eq!(after.cpu.pc, 0x8001);
    assert_eq!(after.cpu.instruction_counter, 1);
    assert!(after.cpu.clocks > before.cpu.clocks);
}

#[test]
fn frontend_runtime_can_pause_and_step_a_single_frame() {
    let rom = make_ines_with_reset_vector(0x8000);
    let mut runtime =
        FrontendRuntime::from_rom_bytes(&rom).expect("test ROM should load into runtime");

    runtime.set_mode(RunMode::Paused);
    let before = runtime.snapshot().debug;

    let paused = runtime.step(FrontendInput::default());
    assert_eq!(paused.status.mode, RunMode::Paused);
    assert_eq!(paused.status.executed, super::ExecutionTarget::None);
    assert_eq!(paused.debug.ppu.frame, before.ppu.frame);

    let stepped = runtime.step(FrontendInput {
        step_frame: true,
        ..FrontendInput::default()
    });
    assert_eq!(stepped.status.mode, RunMode::Paused);
    assert_eq!(stepped.status.executed, super::ExecutionTarget::Frame);
    assert!(stepped.debug.ppu.frame > before.ppu.frame);
}

#[test]
fn frontend_runtime_toggle_pause_switches_modes() {
    let rom = make_ines_with_reset_vector(0x8000);
    let mut runtime =
        FrontendRuntime::from_rom_bytes(&rom).expect("test ROM should load into runtime");

    let paused = runtime.step(FrontendInput {
        toggle_pause: true,
        ..FrontendInput::default()
    });
    assert_eq!(paused.status.mode, RunMode::Paused);

    let running = runtime.step(FrontendInput {
        toggle_pause: true,
        ..FrontendInput::default()
    });
    assert_eq!(running.status.mode, RunMode::Running);
}

#[test]
fn save_state_round_trip_restores_debug_snapshot_and_video_output() {
    let rom = make_ines_with_reset_vector(0x8000);
    let mut runtime =
        FrontendRuntime::from_rom_bytes(&rom).expect("test ROM should load into runtime");

    runtime.step(FrontendInput {
        controller1: ControllerState::from_bits(0x81),
        ..FrontendInput::default()
    });
    runtime.step(FrontendInput::default());

    let expected = runtime.snapshot();
    let expected_debug = expected.debug;
    let expected_frame_number = expected.video.frame_number;
    let expected_pixels = expected.video.pixels.to_vec();
    let state = runtime
        .save_state()
        .expect("runtime with loaded cartridge should save");

    runtime.step(FrontendInput {
        reset: true,
        ..FrontendInput::default()
    });
    runtime.step(FrontendInput {
        step_cpu_instruction: true,
        ..FrontendInput::default()
    });

    runtime
        .load_state(&state)
        .expect("saved state should load back into the same runtime");
    let restored = runtime.snapshot();

    assert_eq!(restored.debug, expected_debug);
    assert_eq!(restored.video.frame_number, expected_frame_number);
    assert_eq!(restored.video.pixels, expected_pixels.as_slice());
}

#[test]
#[ignore = "ROM smoke test for MMC1 game boot output"]
fn rockman2_mmc1_rom_boot_frame_matches_reference_hash() {
    assert_rom_boot_frame_hash(
        "roms/mmc1/Rockman2(J).nes",
        180,
        0xE2272AE0D688020E,
        "out/failed-rockman2-boot.ppm",
    );
}

#[test]
#[ignore = "ROM smoke test for UxROM game boot output"]
fn ducktales_uxrom_rom_boot_frame_matches_reference_hash() {
    assert_rom_boot_frame_hash(
        "roms/uxrom/DuckTales(E).nes",
        180,
        0x697513C749EAE77E,
        "out/failed-ducktales-boot.ppm",
    );
}

#[test]
#[ignore = "ROM smoke test for MMC3 game boot output"]
fn supercontra_mmc3_rom_boot_frame_matches_reference_hash() {
    assert_rom_boot_frame_hash(
        "roms/mmc3/SuperContra(U).nes",
        180,
        0xA1C967E9F594C010,
        "out/failed-supercontra-boot.ppm",
    );
}

#[test]
#[ignore = "long-running nestest automation ROM validation"]
fn nestest_automation_mode_reports_zero_error_bytes() {
    let Some(rom) = read_optional_binary_fixture("roms/nestest/nestest.nes") else {
        return;
    };
    let Some(log) = read_optional_text_fixture("roms/nestest/nestest.log") else {
        return;
    };
    let trace_line_count = log.lines().filter(|line| !line.trim().is_empty()).count();
    let mut nes = NES::new();

    nes.load_cartridge_ines(&rom)
        .expect("nestest ROM should load as NROM");
    nes.bus.ram[0x0002] = 0x00;
    nes.bus.ram[0x0003] = 0x00;
    nes.cpu.init_nestest_state_for_test();

    for _ in 0..trace_line_count {
        nes.cpu.step_instruction_for_test(&mut nes.bus);
    }

    assert_eq!(
        (nes.bus.ram[0x0002], nes.bus.ram[0x0003]),
        (0x00, 0x00),
        "nestest reported failure codes 02h={:02X}, 03h={:02X}, terminal pc={:04X}, clocks={}",
        nes.bus.ram[0x0002],
        nes.bus.ram[0x0003],
        nes.cpu.pc(),
        nes.cpu.clocks()
    );
}

#[test]
#[ignore = "long-running nestest trace comparison against reference log"]
fn nestest_trace_matches_reference_log() {
    let Some(rom) = read_optional_binary_fixture("roms/nestest/nestest.nes") else {
        return;
    };
    let Some(log) = read_optional_text_fixture("roms/nestest/nestest.log") else {
        return;
    };
    let mut nes = NES::new();

    nes.load_cartridge_ines(&rom)
        .expect("nestest ROM should load as NROM");
    nes.cpu.init_nestest_state_for_test();

    for (line_number, line) in log.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let expected = parse_nestest_trace_line(line);
        let (pc, a, x, y, p, sp, cyc) = nes.cpu.trace_state_for_test();
        let actual = NestestTraceEntry {
            pc,
            a,
            x,
            y,
            p,
            sp,
            cyc,
        };

        assert_eq!(
            actual,
            expected,
            "nestest trace diverged at line {}",
            line_number + 1
        );

        nes.cpu.step_instruction_for_test(&mut nes.bus);
    }
}

#[test]
#[ignore = "long-running AccuracyCoin page test"]
fn accuracy_coin_page_one_b_flag_passes() {
    const RESULT_ADDRS: [u16; 1] = [0x0475];
    const RESULT_NAMES: [&str; 1] = ["The B Flag"];

    let Some((nes, results)) = run_accuracy_coin_page(0, &RESULT_ADDRS) else {
        return;
    };

    let failures: Vec<String> = results
        .iter()
        .enumerate()
        .filter(|(_, result)| result.status != 1)
        .map(|(index, result)| {
            format!(
                "{} @ ${:04X}: raw={:02X} status={} error={:02X}",
                RESULT_NAMES[index],
                RESULT_ADDRS[index],
                result.raw,
                result.status,
                result.error_code
            )
        })
        .collect();

    assert!(
        failures.is_empty(),
        "AccuracyCoin page 1 B Flag test reported failures.\nterminal pc={:04X}, clocks={}\n{}",
        nes.cpu.pc(),
        nes.cpu.clocks(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "long-running AccuracyCoin page test"]
fn accuracy_coin_page_one_cpu_bus_side_effects_pass() {
    const RESULT_ADDRS: [u16; 4] = [0x0406, 0x0407, 0x0408, 0x047D];
    const RESULT_NAMES: [&str; 4] = [
        "Dummy read cycles",
        "Dummy write cycles",
        "Open Bus",
        "All NOP instructions",
    ];

    let Some((nes, results)) = run_accuracy_coin_page(0, &RESULT_ADDRS) else {
        return;
    };

    let failures: Vec<String> = results
        .iter()
        .enumerate()
        .filter(|(_, result)| result.status != 1)
        .map(|(index, result)| {
            format!(
                "{} @ ${:04X}: raw={:02X} status={} error={:02X}",
                RESULT_NAMES[index],
                RESULT_ADDRS[index],
                result.raw,
                result.status,
                result.error_code
            )
        })
        .collect();

    assert!(
        failures.is_empty(),
        "AccuracyCoin page 1 CPU bus side-effect tests reported failures.\nterminal pc={:04X}, clocks={}\n{}",
        nes.cpu.pc(),
        nes.cpu.clocks(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "long-running AccuracyCoin page test"]
fn accuracy_coin_page_three_frame_counter_irq_passes() {
    const RESULT_ADDRS: [u16; 1] = [0x0467];
    const RESULT_NAMES: [&str; 1] = ["Frame Counter IRQ"];

    let Some((nes, results)) = run_accuracy_coin_page(13, &RESULT_ADDRS) else {
        return;
    };

    let failures: Vec<String> = results
        .iter()
        .enumerate()
        .filter(|(_, result)| result.status != 1)
        .map(|(index, result)| {
            format!(
                "{} @ ${:04X}: raw={:02X} status={} error={:02X}",
                RESULT_NAMES[index],
                RESULT_ADDRS[index],
                result.raw,
                result.status,
                result.error_code
            )
        })
        .collect();

    assert!(
        failures.is_empty(),
        "AccuracyCoin page 3 frame counter IRQ test reported failures.\nterminal pc={:04X}, clocks={}\n{}",
        nes.cpu.pc(),
        nes.cpu.clocks(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "long-running AccuracyCoin page test"]
fn accuracy_coin_page_two_addressing_mode_wraparound_passes() {
    const RESULT_ADDRS: [u16; 6] = [0x046E, 0x046F, 0x0470, 0x0471, 0x0472, 0x0473];
    const RESULT_NAMES: [&str; 6] = [
        "Absolute Indexed Wraparound",
        "Zero Page Indexed Wraparound",
        "Indirect Addressing Wraparound",
        "Indirect Addressing, X Wraparound",
        "Indirect Addressing, Y Wraparound",
        "Relative Addressing Wraparound",
    ];

    let Some((nes, results)) = run_accuracy_coin_page(1, &RESULT_ADDRS) else {
        return;
    };

    let failures: Vec<String> = results
        .iter()
        .enumerate()
        .filter(|(_, result)| result.status != 1)
        .map(|(index, result)| {
            format!(
                "{} @ ${:04X}: raw={:02X} status={} error={:02X}",
                RESULT_NAMES[index],
                RESULT_ADDRS[index],
                result.raw,
                result.status,
                result.error_code
            )
        })
        .collect();

    assert!(
        failures.is_empty(),
        "AccuracyCoin page 2 reported failures after booting to the menu and running the page.\nterminal pc={:04X}, clocks={}\n{}",
        nes.cpu.pc(),
        nes.cpu.clocks(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "long-running AccuracyCoin page test"]
fn accuracy_coin_page_eleven_ane_immediate_passes() {
    const RESULT_ADDRS: [u16; 1] = [0x0414];
    const RESULT_NAMES: [&str; 1] = ["ANE Immediate"];

    let Some((nes, results)) = run_accuracy_coin_page(10, &RESULT_ADDRS) else {
        return;
    };

    let failures: Vec<String> = results
        .iter()
        .enumerate()
        .filter(|(_, result)| result.status != 1)
        .map(|(index, result)| {
            format!(
                "{} @ ${:04X}: raw={:02X} status={} error={:02X}",
                RESULT_NAMES[index],
                RESULT_ADDRS[index],
                result.raw,
                result.status,
                result.error_code
            )
        })
        .collect();

    assert!(
        failures.is_empty(),
        "AccuracyCoin page 11 ANE immediate test reported failures.\nterminal pc={:04X}, clocks={}\n{}",
        nes.cpu.pc(),
        nes.cpu.clocks(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "long-running AccuracyCoin page test"]
fn accuracy_coin_page_fourteen_delta_modulation_channel_passes() {
    const RESULT_ADDRS: [u16; 1] = [0x046A];
    const RESULT_NAMES: [&str; 1] = ["Delta Modulation Channel"];

    let Some((nes, results)) = run_accuracy_coin_page(13, &RESULT_ADDRS) else {
        return;
    };

    let failures: Vec<String> = results
        .iter()
        .enumerate()
        .filter(|(_, result)| result.status != 1)
        .map(|(index, result)| {
            format!(
                "{} @ ${:04X}: raw={:02X} status={} error={:02X}",
                RESULT_NAMES[index],
                RESULT_ADDRS[index],
                result.raw,
                result.status,
                result.error_code
            )
        })
        .collect();

    assert!(
        failures.is_empty(),
        "AccuracyCoin page 14 delta modulation channel test reported failures.\nterminal pc={:04X}, clocks={}\n{}",
        nes.cpu.pc(),
        nes.cpu.clocks(),
        failures.join("\n")
    );
}
