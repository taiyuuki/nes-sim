use super::*;
use crate::bus::CPUBus;
use crate::cheat::CheatTable;
use crate::{CoreCommand, CoreEvent, NES};

// ---- Game Genie ----------------------------------------------------------

/// 向量来自编码公式的手工推导 + 与 Nestopia/FCEUX 参考实现逐位核对。
#[test]
fn game_genie_known_vectors() {
    let c = parse_game_genie("AAAAAA").unwrap();
    assert_eq!(c.address, 0x8000);
    assert_eq!(c.value, 0x00);
    assert_eq!(c.compare, None);

    let c = parse_game_genie("NYAAAE").unwrap();
    assert_eq!(c.address, 0x8000);
    assert_eq!(c.value, 0xFF);
    assert_eq!(c.compare, None);

    let c = parse_game_genie("ZPEAAAGL").unwrap();
    assert_eq!(c.address, 0x8000);
    assert_eq!(c.value, 0x12);
    assert_eq!(c.compare, Some(0x34));
}

#[test]
fn game_genie_lowercase_and_whitespace() {
    let c = parse_game_genie("  zpeaaagl ").unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x8000, 0x12, Some(0x34)));
}

#[test]
fn game_genie_rejects_invalid() {
    assert_eq!(
        parse_game_genie("").unwrap_err(),
        CheatError::InvalidGameGenie
    );
    assert_eq!(
        parse_game_genie("SXIOPOO").unwrap_err(),
        CheatError::InvalidGameGenie
    ); // 7 字符
    assert_eq!(
        parse_game_genie("SXMOP").unwrap_err(),
        CheatError::InvalidGameGenie
    ); // M 不在字母表
}

#[test]
fn game_genie_roundtrip() {
    let addresses = [0x8000, 0x8421, 0xA5A5, 0xBEEF, 0xC000, 0xFEED, 0xFFFF];
    let values = [0x00, 0x0F, 0x10, 0x55, 0xAA, 0xFF];
    let compares = [None, Some(0x00), Some(0x38), Some(0xE7), Some(0xFF)];
    for &address in &addresses {
        for &value in &values {
            for &compare in &compares {
                let code = encode_game_genie(address, value, compare).unwrap();
                let parsed = parse_game_genie(&code).unwrap();
                assert_eq!(parsed.address, address, "code {code}");
                assert_eq!(parsed.value, value, "code {code}");
                assert_eq!(parsed.compare, compare, "code {code}");
                // Cheat::to_game_genie 等价
                let cheat = Cheat::new(address, value).with_compare_opt(compare);
                assert_eq!(cheat.to_game_genie().unwrap(), code);
            }
        }
    }
    assert_eq!(
        encode_game_genie(0x079F, 0x01, None).unwrap_err(),
        CheatError::UnsupportedAddress
    );
}

// ---- Pro Action Replay ---------------------------------------------------

#[test]
fn pro_action_replay_known_vector() {
    // 退化输入（0x8000/0/0）恰好编码为初始密钥本身。
    let code = encode_pro_action_replay(0x8000, 0x00, 0x00).unwrap();
    assert_eq!(code, "FCBDD274");
    let c = parse_pro_action_replay(&code).unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x8000, 0x00, Some(0x00)));
    assert_eq!(
        Cheat::new(0x8000, 0x00)
            .with_compare(0x00)
            .to_pro_action_replay()
            .unwrap(),
        code
    );
}

#[test]
fn pro_action_replay_lowercase_and_invalid() {
    let c = parse_pro_action_replay("fcbdd274").unwrap();
    assert_eq!(c.address, 0x8000);
    assert_eq!(
        parse_pro_action_replay("FCBDD27").unwrap_err(),
        CheatError::InvalidProActionReplay
    );
    assert_eq!(
        parse_pro_action_replay("FCBDD27G").unwrap_err(),
        CheatError::InvalidProActionReplay
    );
    assert_eq!(
        encode_pro_action_replay(0x0700, 0x63, 0x00).unwrap_err(),
        CheatError::UnsupportedAddress
    );
}

#[test]
fn pro_action_replay_roundtrip() {
    let addresses = [0x8000, 0x8421, 0xA5A5, 0xBEEF, 0xC000, 0xFEED, 0xFFFF];
    let values = [0x00, 0x0F, 0x55, 0xAA, 0xFF];
    let compares = [0x00, 0x38, 0xE7, 0xFF];
    for &address in &addresses {
        for &value in &values {
            for &compare in &compares {
                let code = encode_pro_action_replay(address, value, compare).unwrap();
                let parsed = parse_pro_action_replay(&code).unwrap();
                assert_eq!(parsed.address, address, "code {code}");
                assert_eq!(parsed.value, value, "code {code}");
                assert_eq!(parsed.compare, Some(compare), "code {code}");
            }
        }
    }
}

// ---- 单条代码自动识别 ----------------------------------------------------

#[test]
fn parse_code_detects_formats() {
    // VirtuaNES 风格（用户示例）
    let c = parse_code("079F-01-01").unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x079F, 0x01, Some(0x01)));

    // FCEUX 冒号风格
    let c = parse_code("079F:01:01").unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x079F, 0x01, Some(0x01)));

    // 无比较
    let c = parse_code("079F:09").unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x079F, 0x09, None));
    let c = parse_code("079f-09").unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x079F, 0x09, None));

    // 第三段不是十六进制 → 描述
    let c = parse_code("0533:09:9 lives").unwrap();
    assert_eq!(c.compare, None);
    assert_eq!(c.description, "9 lives");
    let c = parse_code("079F-01-01-Inf HP").unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x079F, 0x01, Some(0x01)));
    assert_eq!(c.description, "Inf HP");

    // 短字段宽松解析
    let c = parse_code("79F:1:2").unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x079F, 0x01, Some(0x02)));

    // Game Genie / PAR 自动识别
    let c = parse_code("SXIOPO").unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x91D9, 0xAD, None));
    let c = parse_code("FCBDD274").unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x8000, 0x00, Some(0x00)));

    // FromStr
    let c: Cheat = "079F-01-FF".parse().unwrap();
    assert_eq!((c.address, c.value, c.compare), (0x079F, 0x01, Some(0xFF)));

    // 错误
    assert_eq!(parse_code("   ").unwrap_err(), CheatError::Empty);
    assert_eq!(
        parse_code("hello world").unwrap_err(),
        CheatError::UnknownFormat
    );
    assert_eq!(parse_code("ZZZZ:1").unwrap_err(), CheatError::InvalidRaw);
}

// ---- FCEUX .cht ----------------------------------------------------------

#[test]
fn parses_fceux_cht() {
    let text = "\
0533:09:9 lives\n\
SC079F:01:99:Infinite lives\n\
:0533:08:disabled entry\n\
079F:01:99\n\
not a cheat line\n\
\n\
S079F:02:with S flag\n\
";
    let cheats = parse_fceux_cht(text);
    assert_eq!(cheats.len(), 5);

    assert_eq!(
        (cheats[0].address, cheats[0].value, cheats[0].compare),
        (0x0533, 0x09, None)
    );
    assert_eq!(cheats[0].description, "9 lives");
    assert!(cheats[0].enabled);

    // C 标记 + 描述
    assert_eq!(
        (cheats[1].address, cheats[1].value, cheats[1].compare),
        (0x079F, 0x01, Some(0x99))
    );
    assert_eq!(cheats[1].description, "Infinite lives");

    // ':' 前缀 → 禁用
    assert!(!cheats[2].enabled);

    // 无 C 标记但第三段是十六进制 → 按比较字节处理
    assert_eq!(cheats[3].compare, Some(0x99));

    // S 标记被接受但不影响语义
    assert_eq!((cheats[4].address, cheats[4].value), (0x079F, 0x02));
    assert!(cheats[4].enabled);
}

#[test]
fn fceux_cht_description_may_contain_colons() {
    let cheats = parse_fceux_cht("SC079F:01:99:Infinite: lives");
    assert_eq!(cheats[0].compare, Some(0x99));
    assert_eq!(cheats[0].description, "Infinite: lives");
}

// ---- Nestopia/Mesen XML .cht ----------------------------------------------

const XML_CHT: &str = "\
<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!-- Nestopia cheat file -->\n\
<cheats version=\"1.0\">\n\
  <cheat enabled=\"1\" game=\"D13EED32\">\n\
    <address>0x07FD</address>\n\
    <value>0x63</value>\n\
    <compare>0x00</compare>\n\
    <description>Health &amp; lives</description>\n\
  </cheat>\n\
  <cheat>\n\
    <!-- 内部注释不应中断子元素扫描 -->\n\
    <address>2045</address>\n\
    <value>9</value>\n\
  </cheat>\n\
  <cheat enabled=\"0\">\n\
    <genie>ZPEAAAGL</genie>\n\
    <description>disabled gg</description>\n\
  </cheat>\n\
  <cheat>\n\
    <rocky>FCBDD274</rocky>\n\
  </cheat>\n\
  <cheat>\n\
    <unknown>skip me</unknown>\n\
  </cheat>\n\
  <cheat/>\n\
</cheats>\n\
";

#[test]
fn parses_xml_cht() {
    let cheats = parse_xml_cht(XML_CHT).unwrap();
    assert_eq!(cheats.len(), 4);

    // 十六进制 + 比较 + 实体解码 + CRC 属性
    let c = &cheats[0];
    assert_eq!((c.address, c.value, c.compare), (0x07FD, 0x63, Some(0x00)));
    assert_eq!(c.description, "Health & lives");
    assert_eq!(c.game_crc, Some(0xD13E_ED32));
    assert!(c.enabled);

    // 十进制数值
    let c = &cheats[1];
    assert_eq!((c.address, c.value, c.compare), (2045, 9, None));
    assert_eq!(c.game_crc, None);

    // genie 子元素 + enabled="0"
    let c = &cheats[2];
    assert_eq!((c.address, c.value, c.compare), (0x8000, 0x12, Some(0x34)));
    assert!(!c.enabled);
    assert_eq!(c.description, "disabled gg");

    // rocky 子元素
    let c = &cheats[3];
    assert_eq!((c.address, c.value, c.compare), (0x8000, 0x00, Some(0x00)));
}

#[test]
fn xml_cht_errors_on_non_xml() {
    assert!(matches!(
        parse_xml_cht("0533:09"),
        Err(CheatError::InvalidChtXml(_))
    ));
    assert!(matches!(
        parse_xml_cht("<wrong></wrong>"),
        Err(CheatError::InvalidChtXml(_))
    ));
}

#[test]
fn parse_cht_file_sniffs_format() {
    assert_eq!(parse_cht_file(XML_CHT).unwrap().len(), 4);
    assert_eq!(parse_cht_file("0533:09:9 lives\n").unwrap().len(), 1);
}

// ---- CheatTable ------------------------------------------------------------

#[test]
fn cheat_table_substitution() {
    let mut table = CheatTable::default();
    assert!(!table.is_active());

    let cheats = vec![
        Cheat::new(0x0100, 0x99),
        Cheat::new(0x0200, 0x11).with_compare(0x22),
        Cheat::new(0x0100, 0xAA), // 同地址覆盖，后添加的生效
        Cheat::new(0x0300, 0x77), // 禁用，不参与
    ];
    let mut cheats: Vec<Cheat> = cheats;
    cheats[3].enabled = false;
    table.rebuild(&cheats);
    assert!(table.is_active());

    assert_eq!(table.substitute(0x0100, 0x00), Some(0xAA));
    assert_eq!(table.substitute(0x0200, 0x22), Some(0x11)); // 原值匹配
    assert_eq!(table.substitute(0x0200, 0x33), None); // 原值不匹配 → 透传
    assert_eq!(table.substitute(0x0300, 0x00), None); // 禁用
    assert_eq!(table.substitute(0x0400, 0x00), None); // 未命中
}

// ---- 总线拦截 --------------------------------------------------------------

fn read(bus: &mut crate::bus::NESBus, addr: u16) -> u8 {
    bus.cpu_read(addr)
}

#[test]
fn bus_read_hook_ram_cheat() {
    let mut bus = crate::bus::NESBus::new();
    bus.ram[0x100] = 0x11;
    bus.add_cheat(Cheat::new(0x0100, 0x99));

    assert_eq!(read(&mut bus, 0x0100), 0x99);
    // 写入透过（写入不被拦截），读取仍被替换
    bus.cpu_write(0x0100, 0x55);
    assert_eq!(bus.ram[0x100], 0x55);
    assert_eq!(read(&mut bus, 0x0100), 0x99);

    // Game Genie 硬件语义：按 CPU 精确地址匹配，RAM 镜像不命中
    assert_eq!(read(&mut bus, 0x0900), 0x55);
}

#[test]
fn bus_read_hook_compare_gating() {
    let mut bus = crate::bus::NESBus::new();
    bus.ram[0x100] = 0x11;
    bus.add_cheat(Cheat::new(0x0100, 0x99).with_compare(0x11));

    assert_eq!(read(&mut bus, 0x0100), 0x99);
    bus.ram[0x100] = 0x22;
    assert_eq!(read(&mut bus, 0x0100), 0x22); // 原值不再匹配 → 透传
}

#[test]
fn bus_read_hook_open_bus_latch() {
    let mut bus = crate::bus::NESBus::new();
    bus.add_cheat(Cheat::new(0x0100, 0x99));
    assert_eq!(read(&mut bus, 0x0100), 0x99);
    // CPU 看到的开路总线值应为替换值
    assert_eq!(read(&mut bus, 0x4018), 0x99);
}

#[test]
fn bus_read_hook_disabled_and_toggle() {
    let mut bus = crate::bus::NESBus::new();
    bus.ram[0x100] = 0x11;
    let mut cheat = Cheat::new(0x0100, 0x99);
    cheat.enabled = false;
    bus.add_cheat(cheat);
    assert_eq!(read(&mut bus, 0x0100), 0x11);

    bus.set_cheat_enabled(0, true);
    assert_eq!(read(&mut bus, 0x0100), 0x99);
    bus.set_cheat_enabled(0, false);
    assert_eq!(read(&mut bus, 0x0100), 0x11);

    bus.remove_cheat(0);
    assert!(!bus.remove_cheat(0));
    assert_eq!(read(&mut bus, 0x0100), 0x11);
}

#[test]
fn bus_read_hook_rom_space_without_cartridge() {
    let mut bus = crate::bus::NESBus::new();
    bus.add_cheat(Cheat::new(0x8000, 0x42));
    // 无卡带时 $8000 读到开路总线（0），金手指仍然替换
    assert_eq!(read(&mut bus, 0x8000), 0x42);
}

// ---- NES 端到端 --------------------------------------------------------------

/// 最小 NROM 测试卡带：
/// $8000: LDA #$00 / STA $0700 / LDA $0700 / STA $0050 / JMP $
fn make_test_rom() -> Vec<u8> {
    let mut prg = vec![0u8; 0x4000];
    let program = [
        0xA9, 0x00, // LDA #$00
        0x8D, 0x00, 0x07, // STA $0700
        0xAD, 0x00, 0x07, // LDA $0700
        0x8D, 0x50, 0x00, // STA $0050
        0x4C, 0x0B, 0x80, // JMP $800B
    ];
    prg[..program.len()].copy_from_slice(&program);
    // reset 向量 → $8000（NROM 16KB 在 $C000-$FFFF 处镜像）
    prg[0x3FFC] = 0x00;
    prg[0x3FFD] = 0x80;

    let mut rom = vec![0u8; 16];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1; // PRG 16KB
    rom[5] = 0; // CHR RAM
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 0x2000]);
    rom
}

fn run_one_frame(cheat: Option<Cheat>) -> u8 {
    let mut nes = NES::new();
    let rom = make_test_rom();
    nes.load_cartridge_ines(&rom).unwrap();
    nes.reset();
    if let Some(cheat) = cheat {
        nes.add_cheat(cheat);
    }
    nes.run_frame();
    nes.bus.ram[0x50]
}

#[test]
fn end_to_end_ram_cheat_changes_loaded_value() {
    assert_eq!(run_one_frame(None), 0x00);
    // $0700 原值 0 → 拦截读取返回 0x63
    assert_eq!(run_one_frame(Some(Cheat::new(0x0700, 0x63))), 0x63);
    // 比较字节不匹配 → 透传原值
    assert_eq!(
        run_one_frame(Some(Cheat::new(0x0700, 0x63).with_compare(0xFF))),
        0x00
    );
    // 比较字节匹配 → 替换
    assert_eq!(
        run_one_frame(Some(Cheat::new(0x0700, 0x63).with_compare(0x00))),
        0x63
    );
}

#[test]
fn end_to_end_game_genie_patches_prg_operand() {
    // 对 $8001（LDA #$00 的立即数）打 ROM 补丁 → 程序行为变为把 0x63 写进 $0700
    let code = encode_game_genie(0x8001, 0x63, None).unwrap();
    let cheat = parse_code(&code).unwrap();
    assert_eq!(run_one_frame(Some(cheat)), 0x63);
}

#[test]
fn core_command_cheat_lifecycle() {
    let mut nes = NES::new();
    let response = nes.execute(CoreCommand::AddCheat(Cheat::new(0x0700, 0x63)));
    assert_eq!(response.event, CoreEvent::CheatsUpdated);
    assert_eq!(nes.cheats().len(), 1);

    nes.execute(CoreCommand::SetCheatEnabled {
        index: 0,
        enabled: false,
    });
    assert!(!nes.cheats()[0].enabled);

    nes.execute(CoreCommand::RemoveCheat(0));
    assert!(nes.cheats().is_empty());

    nes.execute(CoreCommand::SetCheats(vec![
        Cheat::new(0x0100, 0x01),
        Cheat::new(0x0200, 0x02),
    ]));
    assert_eq!(nes.cheats().len(), 2);

    nes.execute(CoreCommand::ClearCheats);
    assert!(nes.cheats().is_empty());
}

#[test]
fn cheats_survive_reset_but_not_cartridge_change() {
    let rom = make_test_rom();
    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).unwrap();
    nes.reset();
    nes.add_cheat(Cheat::new(0x0700, 0x63));
    nes.reset();
    assert_eq!(nes.cheats().len(), 1);

    // 重新插卡 → 金手指清空
    let cartridge = crate::cartridge::Cartridge::from_ines(&rom).unwrap();
    nes.insert_cartridge(cartridge);
    assert!(nes.cheats().is_empty());
}

#[test]
fn savestate_does_not_serialize_cheats() {
    let rom = make_test_rom();
    let mut clean = NES::new();
    clean.load_cartridge_ines(&rom).unwrap();
    clean.reset();

    let mut cheated = NES::new();
    cheated.load_cartridge_ines(&rom).unwrap();
    cheated.reset();
    cheated.add_cheat(Cheat::new(0x0700, 0x63));

    assert_eq!(clean.save_state().unwrap(), cheated.save_state().unwrap());
}
