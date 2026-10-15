//! blargg 测试ROM精度回归。
//!
//! 依赖可选fixture目录 `roms/nes-test-roms/`（gitignored）：缺失时套件自动跳过，
//! 存在时逐一运行并按 $6000 状态码判定。
//!
//! 结果协议（blargg test API）：
//! - $6000: 状态字。$80 = 运行中，$81 = 请求主机复位，<$80 = 结束（0 = 通过）
//! - $6001..$6003: 签名 $DE $B0 $61（2007+新版才有；老版以"见过$80后落地"判定）
//! - $6004..: NUL结尾的结果文本
//!
//! 已知失败项集中在 `blargg_known_accuracy_gaps`（#[ignore]），
//! `cargo test -- --ignored` 可运行，作为精度修复的待办清单。
//!
//! 未纳入的套件：
//! - vbl_nmi_timing / branch_timing_tests：2008老格式，结果仅显示在屏幕
//!   （已分别被 ppu_vbl_nmi、instr_timing 取代）
//! - apu_reset：交互式/断电复位行为，无法自动判定
//! - cpu_reset/ram_after_reset：老格式屏幕输出

use super::*;
use crate::bus::CPUBus;

const ROM_ROOT: &str = "roms/nes-test-roms";

enum BlarggOutcome {
    Pass,
    Fail(String),
}

fn read_test_text(nes: &mut NES) -> String {
    let mut bytes = Vec::new();
    for offset in 0..0x200u16 {
        let byte = CPUBus::cpu_read(&mut nes.bus, 0x6004 + offset);
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// 运行单个blargg测试ROM直到出结果或超时。
/// 兼容两种结果协议：新版带$DE$B0$61签名；2004-2006老版无签名，
/// 以"曾观测到$80运行态后降到<$80"为完成条件。
/// 返回None表示fixture缺失（由调用方决定跳过粒度）。
fn run_blargg_rom(rel_path: &str, max_frames: u64) -> Option<BlarggOutcome> {
    let rom = read_optional_binary_fixture(&format!("{ROM_ROOT}/{rel_path}"))?;
    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom)
        .unwrap_or_else(|error| panic!("load {rel_path}: {error}"));
    nes.reset();

    let mut seen_running = false;
    // $81复位为边沿触发：复位后ROM需数帧才会重写$6000，期间陈旧的$81
    // 不能再次触发复位（否则ROM永远无法完成初始化）
    let mut reset_latched = false;
    for frame in 0..max_frames {
        nes.run_frame();
        let sig = [
            CPUBus::cpu_read(&mut nes.bus, 0x6001),
            CPUBus::cpu_read(&mut nes.bus, 0x6002),
            CPUBus::cpu_read(&mut nes.bus, 0x6003),
        ];
        let signed = sig == [0xDE, 0xB0, 0x61];
        let status = CPUBus::cpu_read(&mut nes.bus, 0x6000);
        if status == 0x81 && signed {
            if !reset_latched {
                reset_latched = true;
                // 测试请求主机复位（如cpu_reset系列），复位后继续监测
                nes.reset();
            }
            continue;
        }
        if status != 0x81 {
            reset_latched = false;
        }
        if status == 0x80 {
            seen_running = true;
            continue;
        }
        if signed || seen_running {
            let text = read_test_text(&mut nes);
            return Some(if status == 0 {
                BlarggOutcome::Pass
            } else {
                BlarggOutcome::Fail(format!("{frame}帧后失败 code {status}: {text}"))
            });
        }
    }
    let text = read_test_text(&mut nes);
    Some(BlarggOutcome::Fail(format!(
        "{max_frames}帧超时, 最后输出: {text}"
    )))
}

/// 运行一个套件（多个ROM），任一fixture缺失则整个套件跳过。
fn run_blargg_suite(suite: &str, roms: &[&str], max_frames: u64) {
    let mut failures = Vec::new();
    collect_blargg_suite(&mut failures, suite, roms, max_frames);
    assert!(
        failures.is_empty(),
        "blargg {suite} 失败 {} 项:\n{}\n",
        failures.len(),
        failures.join("\n")
    );
}

/// 汇总运行一个套件的失败项（不assert），供已知差距清单一次性收集全部结果。
fn collect_blargg_suite(failures: &mut Vec<String>, suite: &str, roms: &[&str], max_frames: u64) {
    for rom in roms {
        let rel_path = if suite.is_empty() {
            (*rom).to_string()
        } else {
            format!("{suite}/{rom}")
        };
        match run_blargg_rom(&rel_path, max_frames) {
            None => {
                eprintln!("skipping blargg suite {suite}: fixture missing");
                return;
            }
            Some(BlarggOutcome::Pass) => {}
            Some(BlarggOutcome::Fail(message)) => {
                failures.push(format!("[{rel_path}] {message}"));
            }
        }
    }
}

#[test]
fn blargg_instr_test_v5() {
    run_blargg_suite(
        "instr_test-v5/rom_singles",
        &[
            "01-basics.nes",
            "02-implied.nes",
            "03-immediate.nes",
            "04-zero_page.nes",
            "05-zp_xy.nes",
            "06-absolute.nes",
            "08-ind_x.nes",
            "09-ind_y.nes",
            "10-branches.nes",
            "11-stack.nes",
            "12-jmp_jsr.nes",
            "13-rts.nes",
            "14-rti.nes",
            "15-brk.nes",
            "16-special.nes",
        ],
        3600,
    );
}

#[test]
fn blargg_instr_timing() {
    run_blargg_suite("instr_timing/rom_singles", &["2-branch_timing.nes"], 3600);
}

#[test]
fn blargg_instr_misc() {
    run_blargg_suite(
        "instr_misc/rom_singles",
        &[
            "01-abs_x_wrap.nes",
            "02-branch_wrap.nes",
            "03-dummy_reads.nes",
        ],
        3600,
    );
}

#[test]
fn blargg_cpu_dummy_writes() {
    run_blargg_suite(
        "cpu_dummy_writes",
        &["cpu_dummy_writes_oam.nes", "cpu_dummy_writes_ppumem.nes"],
        3600,
    );
}

#[test]
fn blargg_cpu_exec_space() {
    run_blargg_suite("cpu_exec_space", &["test_cpu_exec_space_apu.nes"], 3600);
}

#[test]
fn blargg_cpu_reset() {
    run_blargg_suite("cpu_reset", &["registers.nes"], 3600);
}

#[test]
fn blargg_cpu_interrupts_v2() {
    run_blargg_suite(
        "cpu_interrupts_v2/rom_singles",
        &["1-cli_latency.nes"],
        7200,
    );
}

#[test]
fn blargg_ppu_vbl_nmi() {
    run_blargg_suite(
        "ppu_vbl_nmi/rom_singles",
        &[
            "01-vbl_basics.nes",
            "03-vbl_clear_time.nes",
            "09-even_odd_frames.nes",
        ],
        3600,
    );
}

#[test]
fn blargg_oam() {
    run_blargg_suite(
        "",
        &[
            "oam_read/oam_read.nes",
            "oam_stress/oam_stress.nes",
            "ppu_open_bus/ppu_open_bus.nes",
            "ppu_read_buffer/test_ppu_read_buffer.nes",
        ],
        3600,
    );
}

#[test]
fn blargg_apu_test() {
    run_blargg_suite(
        "apu_test/rom_singles",
        &[
            "1-len_ctr.nes",
            "2-len_table.nes",
            "3-irq_flag.nes",
            "4-jitter.nes",
            "5-len_timing.nes",
            "6-irq_flag_timing.nes",
            "8-dmc_rates.nes",
        ],
        10800,
    );
}

/// 已知精度差距清单（修复一项就从这里挪回上面的常跑套件）。
/// 汇总收集全部失败项后统一断言；`cargo test -- --ignored` 运行。
#[test]
#[ignore = "已知模拟器精度差距，修复后逐项挪回常跑套件"]
fn blargg_known_accuracy_gaps() {
    let mut failures = Vec::new();
    // CPU: 非官方指令 SHX/SHY (9E/9C)
    collect_blargg_suite(
        &mut failures,
        "instr_test-v5/rom_singles",
        &["07-abs_xy.nes"],
        3600,
    );
    // CPU: NMI/BRK/IRQ交互与时序。诊断: 5-branch_delays_irq的首行PC=00对期望04
    // ——中断识别晚~1周期。真机在指令的特定周期轮询中断且taken分支末拍抑制轮询;
    // 我们只在指令边界轮询(interrupt_delay机制是近似)。1-cli_latency通过=基础延迟正确。
    // 与sprdma同根源: 需周期级中断轮询(周期切片CPU或等价的逐拍轮询层)。
    collect_blargg_suite(
        &mut failures,
        "cpu_interrupts_v2/rom_singles",
        &[
            "2-nmi_and_brk.nes",
            "3-nmi_and_irq.nes",
            "4-irq_and_dma.nes",
            "5-branch_delays_irq.nes",
        ],
        7200,
    );
    // PPU: VBL置位时刻、NMI使能/抑制/开关时序。02-vbl_set_time诊断结论:
    // 真机基准表(测试源码注释)为V自T+00第二列起, 我们晚2个PPU dot; 且结果对
    // 采样窗口平移±4 dot完全不变(读取时标志已实时置位), 决定因素是sync_vbl
    // 锁相相位 + 读清除与dot1置位的竞争(预测清除后PPU实时经过241,1会再次置位)。
    // 下一步: 读sync_vbl.s, 建模read-clear vs dot1-set竞争。
    collect_blargg_suite(
        &mut failures,
        "ppu_vbl_nmi/rom_singles",
        &[
            "02-vbl_set_time.nes",
            "04-nmi_control.nes",
            "05-nmi_timing.nes",
            "06-suppression.nes",
            "07-nmi_on_timing.nes",
            "08-nmi_off_timing.nes",
            "10-even_odd_timing.nes",
        ],
        3600,
    );
    // APU: $4015重启语义在指令粒度量化和真机行为间无解（已实验证明）：
    // - 立即装载+立即取数(Mesen语义) / 立即装载+2~3拍延迟取数 / 边界取数+延迟装载
    //   三种模型在 7-dmc_basics#19 / 8-dmc_rates / sprdma 的失败位置各差±1拍。
    // 根因: exe_inst在指令首拍执行全部总线访问, 写比硬件早offset(3)拍, 读同样早。
    // 解法需CPU按周期切片执行或APU寄存器访问预测层(参考PPU predict_status_timing)。
    collect_blargg_suite(
        &mut failures,
        "apu_test/rom_singles",
        &["7-dmc_basics.nes"],
        10800,
    );
    // DMA: SPR DMA与DMC DMA竞争。亚周期写队列落地后($4015三角已解)剩余:
    // sync精同步循环周期恒为432.0(自然428+DMA停顿2), 真机429+4=433——每迭代
    // 1周期漂移才能扫出bit4清零窗口。实测我们DMC DMA只偷2拍(真机4/3/2/1按落点);
    // 强制恒4拍会破坏rates的sync收敛(它锁定在当前停顿长度上)。
    // 下一步: 按Nestopia DoDMA的落点规则实现(写周期3拍/$4014重叠2/1拍)。
    // 已试: 恒4拍停顿和写周期条件规则(登记每指令写周期绝对时钟)都会破坏rates
    // ——rates精同步的DMA落在纯读延时循环里需4拍, 而halt对齐与STA写周期的
    // 交互需按Nestopia的StealCycles/writeClocks表语义完整建模(halt推迟到
    // 在途指令的写周期完成), 不是简单的落点查表。循环自吸附到字节周期432
    // (停顿吸收余量), 失谐需自然长429+4=433。
    collect_blargg_suite(
        &mut failures,
        "sprdma_and_dmc_dma",
        &["sprdma_and_dmc_dma.nes", "sprdma_and_dmc_dma_512.nes"],
        7200,
    );
    assert!(
        failures.is_empty(),
        "剩余已知精度差距 {} 项:\n{}\n",
        failures.len(),
        failures.join("\n")
    );
}
