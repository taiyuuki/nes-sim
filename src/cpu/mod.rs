use crate::bus::CPUBus;

#[derive(Clone, Copy)]
pub enum AddrMode {
    IMM,
    IMP,
    ZP0,
    ZPX,
    ZPY,
    REL,
    ABS,
    ABX,
    ABY,
    IND,
    IZX,
    IZY,
}

#[derive(Clone, Copy)]
struct ResolvedOperand {
    addr: u16,
    page_crossed: bool,
    dummy_addr: u16,
}

impl ResolvedOperand {
    fn new(addr: u16) -> Self {
        Self {
            addr,
            page_crossed: false,
            dummy_addr: addr,
        }
    }

    fn with_page_cross(base: u16, addr: u16) -> Self {
        Self {
            addr,
            page_crossed: (base & 0xFF00) != (addr & 0xFF00),
            dummy_addr: (base & 0xFF00) | (addr & 0x00FF),
        }
    }
}

impl AddrMode {
    fn read_page_cross_penalty(self, operand: ResolvedOperand) -> u64 {
        u64::from(
            matches!(self, AddrMode::ABX | AddrMode::ABY | AddrMode::IZY) && operand.page_crossed,
        )
    }
}

#[derive(Clone, Copy)]
enum OpCode {
    BRK,
    ORA,
    SLO,
    NOP,
    ASL,
    PHP,
    BPL,
    CLC,
    JSR,
    AND,
    RLA,
    BIT,
    ROL,
    PLP,
    BMI,
    SEC,
    RTI,
    EOR,
    LSR,
    PHA,
    JMP,
    BVC,
    CLI,
    RTS,
    ADC,
    RRA,
    ROR,
    PLA,
    BVS,
    SEI,
    STA,
    SAX,
    STY,
    STX,
    DEY,
    TXA,
    BCC,
    TYA,
    TXS,
    LDY,
    LDA,
    LDX,
    LAX,
    TAY,
    TAX,
    BCS,
    CLV,
    TSX,
    CPY,
    CMP,
    DCP,
    DEC,
    INY,
    DEX,
    BNE,
    CLD,
    CPX,
    SBC,
    ISC,
    INC,
    INX,
    BEQ,
    SED,
    ANC,
    SRE,
    ALR,
    ARR,
    XAA,
    AXS,
    AHX,
    SHX,
    SHY,
    TAS,
    LAS,
    KIL,
}

#[derive(Clone, Copy)]
struct Inst(OpCode, AddrMode, u8); // opcode, addressing mode, cycles

const INST_SET: [Inst; 256] = [
    Inst(OpCode::BRK, AddrMode::IMP, 7),
    Inst(OpCode::ORA, AddrMode::IZX, 6),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::SLO, AddrMode::IZX, 8),
    Inst(OpCode::NOP, AddrMode::ZP0, 3),
    Inst(OpCode::ORA, AddrMode::ZP0, 3),
    Inst(OpCode::ASL, AddrMode::ZP0, 5),
    Inst(OpCode::SLO, AddrMode::ZP0, 5),
    Inst(OpCode::PHP, AddrMode::IMP, 3),
    Inst(OpCode::ORA, AddrMode::IMM, 2),
    Inst(OpCode::ASL, AddrMode::IMP, 2),
    Inst(OpCode::ANC, AddrMode::IMP, 2),
    Inst(OpCode::NOP, AddrMode::ABS, 4),
    Inst(OpCode::ORA, AddrMode::ABS, 4),
    Inst(OpCode::ASL, AddrMode::ABS, 6),
    Inst(OpCode::SLO, AddrMode::ABS, 6),
    Inst(OpCode::BPL, AddrMode::REL, 2),
    Inst(OpCode::ORA, AddrMode::IZY, 5),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::SLO, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::ORA, AddrMode::ZPX, 4),
    Inst(OpCode::ASL, AddrMode::ZPX, 6),
    Inst(OpCode::SLO, AddrMode::ZPX, 6),
    Inst(OpCode::CLC, AddrMode::IMP, 2),
    Inst(OpCode::ORA, AddrMode::ABY, 4),
    Inst(OpCode::NOP, AddrMode::IMP, 2),
    Inst(OpCode::SLO, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::ORA, AddrMode::ABX, 4),
    Inst(OpCode::ASL, AddrMode::ABX, 7),
    Inst(OpCode::SLO, AddrMode::ABX, 7),
    Inst(OpCode::JSR, AddrMode::ABS, 6),
    Inst(OpCode::AND, AddrMode::IZX, 6),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::RLA, AddrMode::IZX, 8),
    Inst(OpCode::BIT, AddrMode::ZP0, 3),
    Inst(OpCode::AND, AddrMode::ZP0, 3),
    Inst(OpCode::ROL, AddrMode::ZP0, 5),
    Inst(OpCode::RLA, AddrMode::ZP0, 5),
    Inst(OpCode::PLP, AddrMode::IMP, 4),
    Inst(OpCode::AND, AddrMode::IMM, 2),
    Inst(OpCode::ROL, AddrMode::IMP, 2),
    Inst(OpCode::ANC, AddrMode::IMP, 2),
    Inst(OpCode::BIT, AddrMode::ABS, 4),
    Inst(OpCode::AND, AddrMode::ABS, 4),
    Inst(OpCode::ROL, AddrMode::ABS, 6),
    Inst(OpCode::RLA, AddrMode::ABS, 6),
    Inst(OpCode::BMI, AddrMode::REL, 2),
    Inst(OpCode::AND, AddrMode::IZY, 5),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::RLA, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::AND, AddrMode::ZPX, 4),
    Inst(OpCode::ROL, AddrMode::ZPX, 6),
    Inst(OpCode::RLA, AddrMode::ZPX, 6),
    Inst(OpCode::SEC, AddrMode::IMP, 2),
    Inst(OpCode::AND, AddrMode::ABY, 4),
    Inst(OpCode::NOP, AddrMode::IMP, 2),
    Inst(OpCode::RLA, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::AND, AddrMode::ABX, 4),
    Inst(OpCode::ROL, AddrMode::ABX, 7),
    Inst(OpCode::RLA, AddrMode::ABX, 7),
    Inst(OpCode::RTI, AddrMode::IMP, 6),
    Inst(OpCode::EOR, AddrMode::IZX, 6),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::SRE, AddrMode::IZX, 8),
    Inst(OpCode::NOP, AddrMode::ZP0, 3),
    Inst(OpCode::EOR, AddrMode::ZP0, 3),
    Inst(OpCode::LSR, AddrMode::ZP0, 5),
    Inst(OpCode::SRE, AddrMode::ZP0, 5),
    Inst(OpCode::PHA, AddrMode::IMP, 3),
    Inst(OpCode::EOR, AddrMode::IMM, 2),
    Inst(OpCode::LSR, AddrMode::IMP, 2),
    Inst(OpCode::ALR, AddrMode::IMP, 2),
    Inst(OpCode::JMP, AddrMode::ABS, 3),
    Inst(OpCode::EOR, AddrMode::ABS, 4),
    Inst(OpCode::LSR, AddrMode::ABS, 6),
    Inst(OpCode::SRE, AddrMode::ABS, 6),
    Inst(OpCode::BVC, AddrMode::REL, 2),
    Inst(OpCode::EOR, AddrMode::IZY, 5),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::SRE, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::EOR, AddrMode::ZPX, 4),
    Inst(OpCode::LSR, AddrMode::ZPX, 6),
    Inst(OpCode::SRE, AddrMode::ZPX, 6),
    Inst(OpCode::CLI, AddrMode::IMP, 2),
    Inst(OpCode::EOR, AddrMode::ABY, 4),
    Inst(OpCode::NOP, AddrMode::IMP, 2),
    Inst(OpCode::SRE, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::EOR, AddrMode::ABX, 4),
    Inst(OpCode::LSR, AddrMode::ABX, 7),
    Inst(OpCode::SRE, AddrMode::ABX, 7),
    Inst(OpCode::RTS, AddrMode::IMP, 6),
    Inst(OpCode::ADC, AddrMode::IZX, 6),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::RRA, AddrMode::IZX, 8),
    Inst(OpCode::NOP, AddrMode::ZP0, 3),
    Inst(OpCode::ADC, AddrMode::ZP0, 3),
    Inst(OpCode::ROR, AddrMode::ZP0, 5),
    Inst(OpCode::RRA, AddrMode::ZP0, 5),
    Inst(OpCode::PLA, AddrMode::IMP, 4),
    Inst(OpCode::ADC, AddrMode::IMM, 2),
    Inst(OpCode::ROR, AddrMode::IMP, 2),
    Inst(OpCode::ARR, AddrMode::IMP, 2),
    Inst(OpCode::JMP, AddrMode::IND, 5),
    Inst(OpCode::ADC, AddrMode::ABS, 4),
    Inst(OpCode::ROR, AddrMode::ABS, 6),
    Inst(OpCode::RRA, AddrMode::ABS, 6),
    Inst(OpCode::BVS, AddrMode::REL, 2),
    Inst(OpCode::ADC, AddrMode::IZY, 5),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::RRA, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::ADC, AddrMode::ZPX, 4),
    Inst(OpCode::ROR, AddrMode::ZPX, 6),
    Inst(OpCode::RRA, AddrMode::ZPX, 6),
    Inst(OpCode::SEI, AddrMode::IMP, 2),
    Inst(OpCode::ADC, AddrMode::ABY, 4),
    Inst(OpCode::NOP, AddrMode::IMP, 2),
    Inst(OpCode::RRA, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::ADC, AddrMode::ABX, 4),
    Inst(OpCode::ROR, AddrMode::ABX, 7),
    Inst(OpCode::RRA, AddrMode::ABX, 7),
    Inst(OpCode::NOP, AddrMode::IMM, 2),
    Inst(OpCode::STA, AddrMode::IZX, 6),
    Inst(OpCode::NOP, AddrMode::IMM, 2),
    Inst(OpCode::SAX, AddrMode::IZX, 6),
    Inst(OpCode::STY, AddrMode::ZP0, 3),
    Inst(OpCode::STA, AddrMode::ZP0, 3),
    Inst(OpCode::STX, AddrMode::ZP0, 3),
    Inst(OpCode::SAX, AddrMode::ZP0, 3),
    Inst(OpCode::DEY, AddrMode::IMP, 2),
    Inst(OpCode::NOP, AddrMode::IMM, 2),
    Inst(OpCode::TXA, AddrMode::IMP, 2),
    Inst(OpCode::XAA, AddrMode::IMP, 2),
    Inst(OpCode::STY, AddrMode::ABS, 4),
    Inst(OpCode::STA, AddrMode::ABS, 4),
    Inst(OpCode::STX, AddrMode::ABS, 4),
    Inst(OpCode::SAX, AddrMode::ABS, 4),
    Inst(OpCode::BCC, AddrMode::REL, 2),
    Inst(OpCode::STA, AddrMode::IZY, 6),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::AHX, AddrMode::IZY, 6),
    Inst(OpCode::STY, AddrMode::ZPX, 4),
    Inst(OpCode::STA, AddrMode::ZPX, 4),
    Inst(OpCode::STX, AddrMode::ZPY, 4),
    Inst(OpCode::SAX, AddrMode::ZPY, 4),
    Inst(OpCode::TYA, AddrMode::IMP, 2),
    Inst(OpCode::STA, AddrMode::ABY, 5),
    Inst(OpCode::TXS, AddrMode::IMP, 2),
    Inst(OpCode::TAS, AddrMode::ABY, 5),
    Inst(OpCode::SHY, AddrMode::IMP, 2),
    Inst(OpCode::STA, AddrMode::ABX, 5),
    Inst(OpCode::SHX, AddrMode::IMP, 2),
    Inst(OpCode::AHX, AddrMode::IMP, 2),
    Inst(OpCode::LDY, AddrMode::IMM, 2),
    Inst(OpCode::LDA, AddrMode::IZX, 6),
    Inst(OpCode::LDX, AddrMode::IMM, 2),
    Inst(OpCode::LAX, AddrMode::IZX, 6),
    Inst(OpCode::LDY, AddrMode::ZP0, 3),
    Inst(OpCode::LDA, AddrMode::ZP0, 3),
    Inst(OpCode::LDX, AddrMode::ZP0, 3),
    Inst(OpCode::LAX, AddrMode::ZP0, 3),
    Inst(OpCode::TAY, AddrMode::IMP, 2),
    Inst(OpCode::LDA, AddrMode::IMM, 2),
    Inst(OpCode::TAX, AddrMode::IMP, 2),
    Inst(OpCode::LAX, AddrMode::IMP, 2),
    Inst(OpCode::LDY, AddrMode::ABS, 4),
    Inst(OpCode::LDA, AddrMode::ABS, 4),
    Inst(OpCode::LDX, AddrMode::ABS, 4),
    Inst(OpCode::LAX, AddrMode::ABS, 4),
    Inst(OpCode::BCS, AddrMode::REL, 2),
    Inst(OpCode::LDA, AddrMode::IZY, 5),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::LAX, AddrMode::IZY, 5),
    Inst(OpCode::LDY, AddrMode::ZPX, 4),
    Inst(OpCode::LDA, AddrMode::ZPX, 4),
    Inst(OpCode::LDX, AddrMode::ZPY, 4),
    Inst(OpCode::LAX, AddrMode::ZPY, 4),
    Inst(OpCode::CLV, AddrMode::IMP, 2),
    Inst(OpCode::LDA, AddrMode::ABY, 4),
    Inst(OpCode::TSX, AddrMode::IMP, 2),
    Inst(OpCode::LAS, AddrMode::IMP, 2),
    Inst(OpCode::LDY, AddrMode::ABX, 4),
    Inst(OpCode::LDA, AddrMode::ABX, 4),
    Inst(OpCode::LDX, AddrMode::ABY, 4),
    Inst(OpCode::LAX, AddrMode::ABY, 4),
    Inst(OpCode::CPY, AddrMode::IMM, 2),
    Inst(OpCode::CMP, AddrMode::IZX, 6),
    Inst(OpCode::NOP, AddrMode::IMM, 2),
    Inst(OpCode::DCP, AddrMode::IZX, 8),
    Inst(OpCode::CPY, AddrMode::ZP0, 3),
    Inst(OpCode::CMP, AddrMode::ZP0, 3),
    Inst(OpCode::DEC, AddrMode::ZP0, 5),
    Inst(OpCode::DCP, AddrMode::ZP0, 5),
    Inst(OpCode::INY, AddrMode::IMP, 2),
    Inst(OpCode::CMP, AddrMode::IMM, 2),
    Inst(OpCode::DEX, AddrMode::IMP, 2),
    Inst(OpCode::AXS, AddrMode::IMP, 2),
    Inst(OpCode::CPY, AddrMode::ABS, 4),
    Inst(OpCode::CMP, AddrMode::ABS, 4),
    Inst(OpCode::DEC, AddrMode::ABS, 6),
    Inst(OpCode::DCP, AddrMode::ABS, 6),
    Inst(OpCode::BNE, AddrMode::REL, 2),
    Inst(OpCode::CMP, AddrMode::IZY, 5),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::DCP, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::CMP, AddrMode::ZPX, 4),
    Inst(OpCode::DEC, AddrMode::ZPX, 6),
    Inst(OpCode::DCP, AddrMode::ZPX, 6),
    Inst(OpCode::CLD, AddrMode::IMP, 2),
    Inst(OpCode::CMP, AddrMode::ABY, 4),
    Inst(OpCode::NOP, AddrMode::IMP, 2),
    Inst(OpCode::DCP, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::CMP, AddrMode::ABX, 4),
    Inst(OpCode::DEC, AddrMode::ABX, 7),
    Inst(OpCode::DCP, AddrMode::ABX, 7),
    Inst(OpCode::CPX, AddrMode::IMM, 2),
    Inst(OpCode::SBC, AddrMode::IZX, 6),
    Inst(OpCode::NOP, AddrMode::IMM, 2),
    Inst(OpCode::ISC, AddrMode::IZX, 8),
    Inst(OpCode::CPX, AddrMode::ZP0, 3),
    Inst(OpCode::SBC, AddrMode::ZP0, 3),
    Inst(OpCode::INC, AddrMode::ZP0, 5),
    Inst(OpCode::ISC, AddrMode::ZP0, 5),
    Inst(OpCode::INX, AddrMode::IMP, 2),
    Inst(OpCode::SBC, AddrMode::IMM, 2),
    Inst(OpCode::NOP, AddrMode::IMP, 2),
    Inst(OpCode::SBC, AddrMode::IMM, 2),
    Inst(OpCode::CPX, AddrMode::ABS, 4),
    Inst(OpCode::SBC, AddrMode::ABS, 4),
    Inst(OpCode::INC, AddrMode::ABS, 6),
    Inst(OpCode::ISC, AddrMode::ABS, 6),
    Inst(OpCode::BEQ, AddrMode::REL, 2),
    Inst(OpCode::SBC, AddrMode::IZY, 5),
    Inst(OpCode::KIL, AddrMode::IMP, 2),
    Inst(OpCode::ISC, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::SBC, AddrMode::ZPX, 4),
    Inst(OpCode::INC, AddrMode::ZPX, 6),
    Inst(OpCode::ISC, AddrMode::ZPX, 6),
    Inst(OpCode::SED, AddrMode::IMP, 2),
    Inst(OpCode::SBC, AddrMode::ABY, 4),
    Inst(OpCode::NOP, AddrMode::IMP, 2),
    Inst(OpCode::ISC, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::SBC, AddrMode::ABX, 4),
    Inst(OpCode::INC, AddrMode::ABX, 7),
    Inst(OpCode::ISC, AddrMode::ABX, 7),
];

///  7  bit  0
/// ---- ----
/// NV1B DIZC
/// |||| ||||
/// |||| |||+- Carry
/// |||| ||+-- Zero
/// |||| |+--- Interrupt Disable
/// |||| +---- Decimal
/// |||+------ (No CPU effect; see: the B flag)
/// ||+------- (No CPU effect; always pushed as 1)
/// |+-------- Overflow
/// +--------- Negative
struct Flag {
    c: bool, // Carry
    z: bool, // Zero
    i: bool, // Interrutps Disable
    d: bool, // Decimal
    v: bool, // Overflow
    n: bool, // Nagative
}

impl Flag {
    fn new() -> Self {
        Self {
            c: false,
            z: false,
            i: false,
            d: false,
            v: false,
            n: false,
        }
    }
}

pub struct CPU {
    a: u8,
    x: u8,
    y: u8,
    sp: u8,
    pc: u16,
    p: Flag,

    // Timing
    cycles: u64,
    clocks: u64,

    // interrupt
    interrupt: u8,
    interrupt_delay: bool,
    pre_interrupt_delay: bool,
    nmi: bool,
    nmi_prev: bool,
    nmi_next: bool,
}

impl CPU {
    pub fn new() -> Self {
        CPU {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0xFFFC,
            p: Flag::new(),
            cycles: 0,
            clocks: 0,
            interrupt: 0,
            interrupt_delay: false,
            pre_interrupt_delay: false,
            nmi: false,
            nmi_prev: false,
            nmi_next: false,
        }
    }

    pub fn reset(&mut self, bus: &mut impl CPUBus) {
        self.pc = self.reset_vector(bus);
        self.sp = self.sp.wrapping_sub(3);
        self.p.i = true;
        self.interrupt_delay = false;
        self.pre_interrupt_delay = false;
        self.nmi_next = false;
        self.nmi_prev = self.nmi;
    }

    fn reset_vector(&mut self, bus: &mut impl CPUBus) -> u16 {
        bus.cpu_read_u16(0xFFFC)
    }

    fn set_zn(&mut self, data: u8) {
        self.p.z = data == 0;
        self.p.n = (data & 0x80) != 0;
    }

    fn status_byte_for_push(&self, break_flag: bool) -> u8 {
        let c: u8 = if self.p.c { 0x01 } else { 0 };
        let z: u8 = if self.p.z { 0x02 } else { 0 };
        let i: u8 = if self.p.i { 0x04 } else { 0 };
        let d: u8 = if self.p.d { 0x08 } else { 0 };
        let b: u8 = if break_flag { 0x10 } else { 0 };
        let v: u8 = if self.p.v { 0x40 } else { 0 };
        let n: u8 = if self.p.n { 0x80 } else { 0 };
        c | z | i | d | b | v | n | 0x20
    }

    fn set_byte_to_p(&mut self, val: u8) {
        self.p.c = (val & 0x01) != 0;
        self.p.z = (val & 0x02) != 0;
        self.p.i = (val & 0x04) != 0;
        self.p.d = (val & 0x08) != 0;
        // Bit 4 (B) and bit 5 are artifacts of how the status is pushed.
        self.p.v = (val & 0x40) != 0;
        self.p.n = (val & 0x80) != 0;
    }

    fn fetch_byte(&mut self, bus: &mut impl CPUBus) -> u8 {
        let data = bus.cpu_read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        data
    }

    fn fetch_u16(&mut self, bus: &mut impl CPUBus) -> u16 {
        let lo = self.fetch_byte(bus) as u16;
        let hi = self.fetch_byte(bus) as u16;
        (hi << 8) | lo
    }

    fn read_u16_zero_page(&mut self, addr: u8, bus: &mut impl CPUBus) -> u16 {
        let lo = bus.cpu_read(addr as u16) as u16;
        let hi = bus.cpu_read(addr.wrapping_add(1) as u16) as u16;
        (hi << 8) | lo
    }

    fn read_u16_indirect(&mut self, addr: u16, bus: &mut impl CPUBus) -> u16 {
        let lo = bus.cpu_read(addr) as u16;
        // Emulate the 6502 indirect JMP page-wrap bug.
        let hi_addr = (addr & 0xFF00) | (addr.wrapping_add(1) & 0x00FF);
        let hi = bus.cpu_read(hi_addr) as u16;
        (hi << 8) | lo
    }

    fn resolve_operand(
        &mut self,
        mode: AddrMode,
        bus: &mut impl CPUBus,
    ) -> Option<ResolvedOperand> {
        match mode {
            AddrMode::IMM => Some(ResolvedOperand::new(self.imm(bus))),
            AddrMode::IMP => None,
            AddrMode::ZP0 => Some(ResolvedOperand::new(self.zp0(bus))),
            AddrMode::ZPX => Some(ResolvedOperand::new(self.zpx(bus))),
            AddrMode::ZPY => Some(ResolvedOperand::new(self.zpy(bus))),
            AddrMode::REL => Some(ResolvedOperand::new(self.rel(bus))),
            AddrMode::ABS => Some(ResolvedOperand::new(self.abs(bus))),
            AddrMode::ABX => {
                let base = self.fetch_u16(bus);
                let addr = base.wrapping_add(self.x as u16);
                Some(ResolvedOperand::with_page_cross(base, addr))
            }
            AddrMode::ABY => {
                let base = self.fetch_u16(bus);
                let addr = base.wrapping_add(self.y as u16);
                Some(ResolvedOperand::with_page_cross(base, addr))
            }
            AddrMode::IND => Some(ResolvedOperand::new(self.ind(bus))),
            AddrMode::IZX => Some(ResolvedOperand::new(self.izx(bus))),
            AddrMode::IZY => {
                let ptr = self.fetch_byte(bus);
                let base = self.read_u16_zero_page(ptr, bus);
                let addr = base.wrapping_add(self.y as u16);
                Some(ResolvedOperand::with_page_cross(base, addr))
            }
        }
    }

    fn require_operand(&mut self, mode: AddrMode, bus: &mut impl CPUBus) -> ResolvedOperand {
        self.resolve_operand(mode, bus)
            .expect("Instruction requires an operand address")
    }

    fn issue_dummy_read_on_page_cross(
        &mut self,
        operand: ResolvedOperand,
        bus: &mut impl CPUBus,
    ) {
        if operand.page_crossed {
            let _ = bus.cpu_read(operand.dummy_addr);
        }
    }

    fn issue_store_dummy_read(
        &mut self,
        mode: AddrMode,
        operand: ResolvedOperand,
        bus: &mut impl CPUBus,
    ) {
        if matches!(mode, AddrMode::ABX | AddrMode::ABY | AddrMode::IZY) {
            let _ = bus.cpu_read(operand.dummy_addr);
        }
    }

    fn issue_rmw_dummy_read(
        &mut self,
        mode: AddrMode,
        operand: ResolvedOperand,
        bus: &mut impl CPUBus,
    ) {
        if matches!(mode, AddrMode::ABX) {
            let _ = bus.cpu_read_timed(operand.dummy_addr, 4);
        }
    }

    fn issue_nop_bus_read(&mut self, mode: AddrMode, operand: Option<ResolvedOperand>, bus: &mut impl CPUBus) {
        match mode {
            AddrMode::IMP => {
                let _ = bus.cpu_read(self.pc);
            }
            AddrMode::IMM => {}
            _ => {
                let operand = operand.expect("non-implied NOP should have an operand");
                self.issue_dummy_read_on_page_cross(operand, bus);
                let _ = bus.cpu_read(operand.addr);
            }
        }
    }

    fn rmw_memory<F>(&mut self, addr: u16, bus: &mut impl CPUBus, op: F) -> u8
    where
        F: FnOnce(&mut Self, u8) -> u8,
    {
        let value = bus.cpu_read(addr);
        bus.cpu_write(addr, value);
        let result = op(self, value);
        bus.cpu_write(addr, result);
        result
    }

    fn rmw_memory_timed<F>(
        &mut self,
        addr: u16,
        bus: &mut impl CPUBus,
        read_cycle_offset: u8,
        write_old_cycle_offset: u8,
        write_new_cycle_offset: u8,
        op: F,
    ) -> u8
    where
        F: FnOnce(&mut Self, u8) -> u8,
    {
        let value = bus.cpu_read_timed(addr, read_cycle_offset);
        bus.cpu_write_timed(addr, value, write_old_cycle_offset);
        let result = op(self, value);
        bus.cpu_write_timed(addr, result, write_new_cycle_offset);
        result
    }

    fn read_cycle_offset(mode: AddrMode, operand: ResolvedOperand) -> u8 {
        match mode {
            AddrMode::IMM => 1,
            AddrMode::ZP0 => 2,
            AddrMode::ZPX | AddrMode::ZPY => 3,
            AddrMode::ABS => 3,
            AddrMode::ABX | AddrMode::ABY => {
                if operand.page_crossed {
                    4
                } else {
                    3
                }
            }
            AddrMode::IZX => 4,
            AddrMode::IZY => {
                if operand.page_crossed {
                    5
                } else {
                    4
                }
            }
            _ => 0,
        }
    }

    fn store_cycle_offset(mode: AddrMode) -> u8 {
        match mode {
            AddrMode::ZP0 => 2,
            AddrMode::ZPX | AddrMode::ZPY => 3,
            AddrMode::ABS => 3,
            AddrMode::ABX | AddrMode::ABY => 4,
            AddrMode::IZX => 5,
            AddrMode::IZY => 5,
            _ => 0,
        }
    }

    fn rmw_cycle_offsets(mode: AddrMode) -> (u8, u8, u8) {
        match mode {
            AddrMode::ZP0 => (2, 3, 4),
            AddrMode::ZPX => (3, 4, 5),
            AddrMode::ABS => (3, 4, 5),
            AddrMode::ABX => (5, 6, 7),
            _ => (0, 0, 0),
        }
    }

    pub fn exe_inst(&mut self, inst_byte: u8, bus: &mut impl CPUBus) {
        let inst = INST_SET[inst_byte as usize];
        let mut extra_cycles = 0_u64;
        match inst.0 {
            // Do nothing.
            OpCode::NOP => {
                let operand = self.resolve_operand(inst.1, bus);
                if let Some(operand) = operand {
                    extra_cycles += inst.1.read_page_cross_penalty(operand);
                }
                self.issue_nop_bus_read(inst.1, operand, bus);
            }
            OpCode::KIL => {
                let _ = self.resolve_operand(inst.1, bus);
            }
            // Bitwise
            OpCode::ORA => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.ora(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::AND => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.and(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::BIT => {
                let operand = self.require_operand(inst.1, bus);
                self.bit_timed(operand.addr, Self::read_cycle_offset(inst.1, operand), bus);
            }
            // Shift
            OpCode::EOR => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.eor(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::ASL => {
                if matches!(inst.1, AddrMode::IMP) {
                    self.a = self.op_asl(self.a);
                } else {
                    let operand = self.require_operand(inst.1, bus);
                    self.issue_rmw_dummy_read(inst.1, operand, bus);
                    self.asl(operand.addr, bus);
                }
            }
            OpCode::ROL => {
                if matches!(inst.1, AddrMode::IMP) {
                    self.a = self.op_rol(self.a);
                } else {
                    let operand = self.require_operand(inst.1, bus);
                    self.issue_rmw_dummy_read(inst.1, operand, bus);
                    self.rol(operand.addr, bus);
                }
            }
            OpCode::LSR => {
                if matches!(inst.1, AddrMode::IMP) {
                    self.a = self.op_lsr(self.a);
                } else {
                    let operand = self.require_operand(inst.1, bus);
                    self.issue_rmw_dummy_read(inst.1, operand, bus);
                    self.lsr(operand.addr, bus);
                }
            }
            OpCode::ROR => {
                if matches!(inst.1, AddrMode::IMP) {
                    self.a = self.op_ror(self.a);
                } else {
                    let operand = self.require_operand(inst.1, bus);
                    self.issue_rmw_dummy_read(inst.1, operand, bus);
                    self.ror(operand.addr, bus);
                }
            }
            // Access
            OpCode::LDA => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.lda_timed(operand.addr, Self::read_cycle_offset(inst.1, operand), bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::LDX => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.ldx(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::LDY => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.ldy(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::STA => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_store_dummy_read(inst.1, operand, bus);
                self.sta_timed(operand.addr, Self::store_cycle_offset(inst.1), bus);
            }
            OpCode::STX => {
                let operand = self.require_operand(inst.1, bus);
                self.stx(operand.addr, bus);
            }
            OpCode::STY => {
                let operand = self.require_operand(inst.1, bus);
                self.sty(operand.addr, bus);
            }
            // Transfer
            OpCode::TAX => self.tax(),
            OpCode::TXA => self.txa(),
            OpCode::TAY => self.tay(),
            OpCode::TYA => self.tya(),
            // Jump
            OpCode::JMP => {
                let operand = self.require_operand(inst.1, bus);
                self.jmp(operand.addr);
            }
            OpCode::JSR => {
                let operand = self.require_operand(inst.1, bus);
                self.jsr(operand.addr, bus);
            }
            OpCode::RTS => self.rts(bus),
            OpCode::BRK => self.brk(bus),
            OpCode::RTI => self.rti(bus),
            // Flags
            OpCode::CLC => self.clc(),
            OpCode::SEC => self.sec(),
            OpCode::CLI => self.cli(),
            OpCode::SEI => self.sei(),
            OpCode::CLD => self.cld(),
            OpCode::SED => self.sed(),
            OpCode::CLV => self.clv(),
            // Stack
            OpCode::PHA => self.pha(bus),
            OpCode::PLA => self.pla(bus),
            OpCode::PHP => self.php(bus),
            OpCode::PLP => self.plp(bus),
            OpCode::TXS => self.txs(),
            OpCode::TSX => self.tsx(),
            // Compare
            OpCode::CMP => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.cmp(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::CPX => {
                let operand = self.require_operand(inst.1, bus);
                self.cpx(operand.addr, bus);
            }
            OpCode::CPY => {
                let operand = self.require_operand(inst.1, bus);
                self.cpy(operand.addr, bus);
            }
            // Branch
            OpCode::BEQ => {
                let operand = self.require_operand(inst.1, bus);
                extra_cycles += self.beq(operand.addr);
            }
            OpCode::BNE => {
                let operand = self.require_operand(inst.1, bus);
                extra_cycles += self.bne(operand.addr);
            }
            OpCode::BCS => {
                let operand = self.require_operand(inst.1, bus);
                extra_cycles += self.bcs(operand.addr);
            }
            OpCode::BCC => {
                let operand = self.require_operand(inst.1, bus);
                extra_cycles += self.bcc(operand.addr);
            }
            OpCode::BMI => {
                let operand = self.require_operand(inst.1, bus);
                extra_cycles += self.bmi(operand.addr);
            }
            OpCode::BPL => {
                let operand = self.require_operand(inst.1, bus);
                extra_cycles += self.bpl(operand.addr);
            }
            OpCode::BVS => {
                let operand = self.require_operand(inst.1, bus);
                extra_cycles += self.bvs(operand.addr);
            }
            OpCode::BVC => {
                let operand = self.require_operand(inst.1, bus);
                extra_cycles += self.bvc(operand.addr);
            }
            // Arithmetic
            OpCode::INX => self.inx(),
            OpCode::INY => self.iny(),
            OpCode::DEX => self.dex(),
            OpCode::DEY => self.dey(),
            OpCode::INC => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_rmw_dummy_read(inst.1, operand, bus);
                self.inc(operand.addr, bus);
            }
            OpCode::DEC => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_rmw_dummy_read(inst.1, operand, bus);
                self.dec(operand.addr, bus);
            }
            OpCode::ADC => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.adc(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::SBC => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.sbc(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            // unofficial
            OpCode::LAX => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.lax(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::AHX => {
                let operand = self.require_operand(inst.1, bus);
                self.ahx(operand.addr, bus);
            }
            OpCode::ALR => {
                let operand = self.require_operand(inst.1, bus);
                self.alr(operand.addr, bus);
            }
            OpCode::ANC => {
                let operand = self.require_operand(inst.1, bus);
                self.anc(operand.addr, bus);
            }
            OpCode::ARR => {
                let operand = self.require_operand(inst.1, bus);
                self.arr(operand.addr, bus);
            }
            OpCode::AXS => {
                let operand = self.require_operand(inst.1, bus);
                self.axs(operand.addr, bus);
            }
            OpCode::DCP => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_rmw_dummy_read(inst.1, operand, bus);
                self.dcp(operand.addr, bus);
            }
            OpCode::LAS => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_dummy_read_on_page_cross(operand, bus);
                self.las(operand.addr, bus);
            }
            OpCode::ISC => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_rmw_dummy_read(inst.1, operand, bus);
                self.isc(operand.addr, bus);
            }
            OpCode::RLA => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_rmw_dummy_read(inst.1, operand, bus);
                self.rla(operand.addr, bus);
            }
            OpCode::RRA => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_rmw_dummy_read(inst.1, operand, bus);
                self.rra(operand.addr, bus);
            }
            OpCode::SLO => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_rmw_dummy_read(inst.1, operand, bus);
                self.slo_timed(operand.addr, inst.1, bus);
            }
            OpCode::SRE => {
                let operand = self.require_operand(inst.1, bus);
                self.issue_rmw_dummy_read(inst.1, operand, bus);
                self.sre(operand.addr, bus);
            }
            OpCode::TAS => {
                let operand = self.require_operand(inst.1, bus);
                self.tas(operand.addr, bus);
            }
            OpCode::SHX => {
                let operand = self.require_operand(inst.1, bus);
                self.shx(operand.addr, bus);
            }
            OpCode::SHY => {
                let operand = self.require_operand(inst.1, bus);
                self.shy(operand.addr, bus);
            }
            OpCode::SAX => {
                let operand = self.require_operand(inst.1, bus);
                self.sax(operand.addr, bus);
            }
            _ => {
                let _ = self.resolve_operand(inst.1, bus);
            } // Illegal opcode, do nothing
        }
        self.cycles += inst.2 as u64 + extra_cycles;
    }

    pub fn clock(&mut self, bus: &mut impl CPUBus) {
        self.clocks += 1;

        // Latch NMI edge every CPU clock.
        if self.nmi && !self.nmi_prev {
            self.nmi_next = true;
        }
        self.nmi_prev = self.nmi;

        // 周期倒计时
        if self.cycles > 0 {
            self.cycles -= 1;
            if self.cycles > 0 {
                return;
            }
        }

        if bus.try_dma() {
            return;
        }

        if self.nmi_next {
            self.nmi_interrupt(bus);
            self.nmi_next = false;
            self.cycles += 7;
            return;
        }

        if self.interrupt > 0 {
            if self.interrupt_delay {
                self.interrupt_delay = false;
                if !self.pre_interrupt_delay {
                    self.irq_interrupt(bus);
                    self.cycles += 7;
                    return;
                }
            } else if !self.p.i {
                self.irq_interrupt(bus);
                self.cycles += 7;
                return;
            }
        } else {
            self.interrupt_delay = false;
        }

        let inst_byte = bus.cpu_read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        self.exe_inst(inst_byte, bus);
    }

    pub fn set_nmi(&mut self, nmi: bool) {
        self.nmi = nmi;
    }

    pub fn clocks(&self) -> u64 {
        self.clocks
    }

    pub fn pc(&self) -> u16 {
        self.pc
    }

    #[cfg(test)]
    pub(crate) fn init_nestest_state_for_test(&mut self) {
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xFD;
        self.pc = 0xC000;
        self.p = Flag::new();
        self.p.i = true;
        self.cycles = 0;
        self.clocks = 7;
        self.interrupt = 0;
        self.interrupt_delay = false;
        self.pre_interrupt_delay = false;
        self.nmi = false;
        self.nmi_prev = false;
        self.nmi_next = false;
    }

    #[cfg(test)]
    pub(crate) fn trace_state_for_test(&self) -> (u16, u8, u8, u8, u8, u8, u64) {
        let p = self.status_byte_for_push(false) & !0x10;
        (self.pc, self.a, self.x, self.y, p, self.sp, self.clocks)
    }

    #[cfg(test)]
    pub(crate) fn step_instruction_for_test(&mut self, bus: &mut impl CPUBus) {
        let inst_byte = bus.cpu_read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        self.exe_inst(inst_byte, bus);
        self.clocks += self.cycles;
        self.cycles = 0;
    }

    pub fn set_irq(&mut self, irq: bool) {
        self.interrupt = u8::from(irq);
    }

    pub fn irq_raise(&mut self, mask: u8) {
        self.interrupt |= mask;
    }

    pub fn irq_clear(&mut self, mask: u8) {
        self.interrupt &= !mask;
    }

    pub fn irq_set_level(&mut self, mask: u8, active: bool) {
        if active {
            self.irq_raise(mask)
        } else {
            self.irq_clear(mask)
        }
    }

    pub fn irq_pending(&self) -> bool {
        self.interrupt != 0
    }

    #[cfg(test)]
    fn trace_irq(args: std::fmt::Arguments<'_>) {
        if std::env::var_os("NES_TRACE_IRQ").is_some() {
            eprintln!("{args}");
        }
    }

    #[cfg(not(test))]
    fn trace_irq(_args: std::fmt::Arguments<'_>) {}

    fn nmi_interrupt(&mut self, bus: &mut impl CPUBus) {
        self.stack_push((self.pc >> 8) as u8, bus);
        self.stack_push(self.pc as u8, bus);
        let p = self.status_byte_for_push(true) & !0x10;
        self.stack_push(p, bus);
        self.p.i = true;
        self.pc = bus.cpu_read_u16(0xFFFA);
    }

    fn irq_interrupt(&mut self, bus: &mut impl CPUBus) {
        Self::trace_irq(format_args!(
            "irq pc={:04X} x={:02X} clocks={}",
            self.pc, self.x, self.clocks
        ));
        self.stack_push((self.pc >> 8) as u8, bus);
        self.stack_push(self.pc as u8, bus);
        let p = self.status_byte_for_push(true) & !0x10;
        self.stack_push(p, bus);
        self.p.i = true;
        self.pc = bus.cpu_read_u16(0xFFFE);
    }

    fn delay_interrupt(&mut self, previous_i: bool) {
        self.interrupt_delay = true;
        self.pre_interrupt_delay = previous_i;
    }

    fn stack_push(&mut self, val: u8, bus: &mut impl CPUBus) {
        let addr = 0x100 | self.sp as u16;
        bus.cpu_write(addr, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn stack_pop(&mut self, bus: &mut impl CPUBus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        let addr = 0x100 | self.sp as u16;
        bus.cpu_read(addr)
    }

    fn page_crossed(base: u16, target: u16) -> bool {
        (base & 0xFF00) != (target & 0xFF00)
    }

    // === Addressing mode implementations ===
    // fn imp(&mut self, _bus: &mut impl CPUBus) -> u16 {
    //     0 // Implied addressing mode doesn't use an address
    // }

    fn imm(&mut self, _bus: &mut impl CPUBus) -> u16 {
        let addr = self.pc;
        self.pc = self.pc.wrapping_add(1);
        addr
    }

    fn zp0(&mut self, bus: &mut impl CPUBus) -> u16 {
        self.fetch_byte(bus) as u16
    }

    fn zpx(&mut self, bus: &mut impl CPUBus) -> u16 {
        self.fetch_byte(bus).wrapping_add(self.x) as u16
    }

    fn zpy(&mut self, bus: &mut impl CPUBus) -> u16 {
        self.fetch_byte(bus).wrapping_add(self.y) as u16
    }

    fn rel(&mut self, bus: &mut impl CPUBus) -> u16 {
        let offset = self.fetch_byte(bus) as i8;
        self.pc.wrapping_add_signed(offset as i16)
    }

    fn abs(&mut self, bus: &mut impl CPUBus) -> u16 {
        self.fetch_u16(bus)
    }

    fn ind(&mut self, bus: &mut impl CPUBus) -> u16 {
        let ptr = self.fetch_u16(bus);
        self.read_u16_indirect(ptr, bus)
    }

    fn izx(&mut self, bus: &mut impl CPUBus) -> u16 {
        let ptr = self.fetch_byte(bus).wrapping_add(self.x);
        self.read_u16_zero_page(ptr, bus)
    }

    // === Instruction implementations ===
    fn ora(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.ora_value(bus.cpu_read(addr));
    }

    fn ora_value(&mut self, value: u8) {
        self.a |= value;
        self.set_zn(self.a);
    }

    fn and(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.and_value(bus.cpu_read(addr));
    }

    fn and_value(&mut self, value: u8) {
        self.a &= value;
        self.set_zn(self.a);
    }

    fn eor(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.eor_value(bus.cpu_read(addr));
    }

    fn eor_value(&mut self, value: u8) {
        self.a ^= value;
        self.set_zn(self.a);
    }

    fn lda(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.a = bus.cpu_read(addr);
        self.set_zn(self.a);
    }

    fn lda_timed(&mut self, addr: u16, cycle_offset: u8, bus: &mut impl CPUBus) {
        self.a = bus.cpu_read_timed(addr, cycle_offset);
        self.set_zn(self.a);
    }

    fn ldx(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.x = bus.cpu_read(addr);
        self.set_zn(self.x);
    }

    fn ldy(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.y = bus.cpu_read(addr);
        self.set_zn(self.y);
    }

    fn sta(&mut self, addr: u16, bus: &mut impl CPUBus) {
        bus.cpu_write(addr, self.a);
    }

    fn sta_timed(&mut self, addr: u16, cycle_offset: u8, bus: &mut impl CPUBus) {
        bus.cpu_write_timed(addr, self.a, cycle_offset);
    }

    fn stx(&mut self, addr: u16, bus: &mut impl CPUBus) {
        bus.cpu_write(addr, self.x);
    }

    fn sty(&mut self, addr: u16, bus: &mut impl CPUBus) {
        bus.cpu_write(addr, self.y);
    }

    fn tax(&mut self) {
        self.x = self.a;
        self.set_zn(self.x);
    }

    fn txa(&mut self) {
        self.a = self.x;
        self.set_zn(self.a);
    }

    fn tay(&mut self) {
        self.y = self.a;
        self.set_zn(self.y);
    }

    fn tya(&mut self) {
        self.a = self.y;
        self.set_zn(self.a);
    }

    fn inx(&mut self) {
        self.x = self.x.wrapping_add(1);
        self.set_zn(self.x);
    }

    fn dex(&mut self) {
        self.x = self.x.wrapping_sub(1);
        self.set_zn(self.x);
    }

    fn iny(&mut self) {
        self.y = self.y.wrapping_add(1);
        self.set_zn(self.y);
    }

    fn dey(&mut self) {
        self.y = self.y.wrapping_sub(1);
        self.set_zn(self.y);
    }

    fn inc(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let _ = self.rmw_memory(addr, bus, |cpu, value| {
            let result = value.wrapping_add(1);
            cpu.set_zn(result);
            result
        });
    }

    fn dec(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let _ = self.rmw_memory(addr, bus, |cpu, value| {
            let result = value.wrapping_sub(1);
            cpu.set_zn(result);
            result
        });
    }

    // 直接跳到目标地址
    fn jmp(&mut self, addr: u16) {
        self.pc = addr;
    }

    // 跳到目标地址，但要先将当前地址压入栈，栈方向是向下的，所以要先减一
    fn jsr(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let return_addr = self.pc.wrapping_sub(1);
        let _ = bus.cpu_read(0x0100 | self.sp as u16);
        self.stack_push((return_addr >> 8) as u8, bus);
        self.stack_push(return_addr as u8, bus);
        let _ = bus.cpu_read(self.pc.wrapping_sub(1));
        self.pc = addr;
    }

    // 从栈中弹出之前的地址并返回该地址
    fn rts(&mut self, bus: &mut impl CPUBus) {
        let lo = self.stack_pop(bus) as u16;
        let hi = self.stack_pop(bus) as u16;
        self.pc = ((hi << 8) | lo).wrapping_add(1);
    }

    fn brk(&mut self, bus: &mut impl CPUBus) {
        self.pc = self.pc.wrapping_add(1);
        self.stack_push((self.pc >> 8) as u8, bus);
        self.stack_push(self.pc as u8, bus);
        self.stack_push(self.status_byte_for_push(true), bus);
        self.p.i = true;
        self.pc = bus.cpu_read_u16(0xFFFE);
    }

    fn rti(&mut self, bus: &mut impl CPUBus) {
        let val = self.stack_pop(bus);
        self.set_byte_to_p(val);
        let lo = self.stack_pop(bus) as u16;
        let hi = (self.stack_pop(bus) as u16) << 8;
        self.pc = lo | hi;
    }

    fn clc(&mut self) {
        self.p.c = false;
    }

    fn sec(&mut self) {
        self.p.c = true;
    }

    fn cli(&mut self) {
        let old_i = self.p.i;
        self.p.i = false;
        self.delay_interrupt(old_i);
    }

    fn sei(&mut self) {
        let old_i = self.p.i;
        self.p.i = true;
        self.delay_interrupt(old_i);
    }

    fn cld(&mut self) {
        self.p.d = false;
    }

    fn sed(&mut self) {
        self.p.d = true;
    }

    fn clv(&mut self) {
        self.p.v = false;
    }

    fn pha(&mut self, bus: &mut impl CPUBus) {
        self.stack_push(self.a, bus);
    }

    fn pla(&mut self, bus: &mut impl CPUBus) {
        self.a = self.stack_pop(bus);
        self.set_zn(self.a);
    }

    fn php(&mut self, bus: &mut impl CPUBus) {
        let val = self.status_byte_for_push(true);
        self.stack_push(val, bus);
    }

    fn plp(&mut self, bus: &mut impl CPUBus) {
        let old_i = self.p.i;
        let val = self.stack_pop(bus);
        self.set_byte_to_p(val);
        self.delay_interrupt(old_i);
    }

    fn txs(&mut self) {
        self.sp = self.x;
    }

    fn tsx(&mut self) {
        self.x = self.sp;
        self.set_zn(self.x);
    }

    fn cmp_core(&mut self, reg: u8, data: u8) {
        self.p.c = reg >= data;
        self.p.z = reg == data;
        self.p.n = (reg.wrapping_sub(data) & 0x80) != 0;
    }

    fn cmp(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.cmp_value(self.a, bus.cpu_read(addr));
    }

    fn cpx(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.cmp_value(self.x, bus.cpu_read(addr));
    }

    fn cpy(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.cmp_value(self.y, bus.cpu_read(addr));
    }

    fn cmp_value(&mut self, reg: u8, value: u8) {
        self.cmp_core(reg, value);
    }

    fn op_branch(&mut self, addr: u16, flag: bool) -> u64 {
        if flag {
            let old_pc = self.pc;
            self.pc = addr;
            return 1 + u64::from(Self::page_crossed(old_pc, addr));
        }
        0
    }

    fn beq(&mut self, addr: u16) -> u64 {
        self.op_branch(addr, self.p.z)
    }

    fn bne(&mut self, addr: u16) -> u64 {
        self.op_branch(addr, !self.p.z)
    }

    fn bcs(&mut self, addr: u16) -> u64 {
        self.op_branch(addr, self.p.c)
    }

    fn bcc(&mut self, addr: u16) -> u64 {
        self.op_branch(addr, !self.p.c)
    }

    fn bmi(&mut self, addr: u16) -> u64 {
        self.op_branch(addr, self.p.n)
    }

    fn bpl(&mut self, addr: u16) -> u64 {
        self.op_branch(addr, !self.p.n)
    }

    fn bvs(&mut self, addr: u16) -> u64 {
        self.op_branch(addr, self.p.v)
    }

    fn bvc(&mut self, addr: u16) -> u64 {
        self.op_branch(addr, !self.p.v)
    }

    fn adc(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.adc_value(bus.cpu_read(addr));
    }

    fn adc_value(&mut self, val: u8) {
        let carry_in = u8::from(self.p.c);

        let sum = self.a as u16 + val as u16 + carry_in as u16;
        let result = sum as u8;

        self.p.c = sum > 0xFF;
        self.p.z = result == 0;
        self.p.n = (result & 0x80) != 0;

        self.p.v = ((self.a ^ result) & (val ^ result) & 0x80) != 0;

        self.a = result;
    }

    fn sbc(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.sbc_value(bus.cpu_read(addr));
    }

    fn sbc_value(&mut self, val: u8) {
        let val = val ^ 0xFF;
        let carry_in = u8::from(self.p.c);
        let sum = self.a as u16 + val as u16 + carry_in as u16;
        let result = sum as u8;

        self.p.c = sum > 0xFF;
        self.p.z = result == 0;
        self.p.n = (result & 0x80) != 0;
        self.p.v = ((self.a ^ result) & (val ^ result) & 0x80) != 0;

        self.a = result;
    }

    fn op_asl(&mut self, val: u8) -> u8 {
        self.p.c = val & 0x80 != 0;
        let result = val << 1;
        self.set_zn(result);
        result
    }

    fn op_lsr(&mut self, val: u8) -> u8 {
        self.p.c = val & 0x01 != 0;
        let result = val >> 1;
        self.set_zn(result);
        result
    }

    fn op_rol(&mut self, val: u8) -> u8 {
        let c_in = u8::from(self.p.c);
        self.p.c = val & 0x80 != 0;
        let result = (val << 1) | c_in;
        self.set_zn(result);
        result
    }

    fn op_ror(&mut self, val: u8) -> u8 {
        let c_in = u8::from(self.p.c) << 7;
        self.p.c = val & 0x01 != 0;
        let result = (val >> 1) | c_in;
        self.set_zn(result);
        result
    }

    fn asl(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let _ = self.rmw_memory(addr, bus, |cpu, value| cpu.op_asl(value));
    }

    fn lsr(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let _ = self.rmw_memory(addr, bus, |cpu, value| cpu.op_lsr(value));
    }

    fn rol(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let _ = self.rmw_memory(addr, bus, |cpu, value| cpu.op_rol(value));
    }

    fn ror(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let _ = self.rmw_memory(addr, bus, |cpu, value| cpu.op_ror(value));
    }

    fn bit(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let val = bus.cpu_read(addr);
        self.p.z = self.a & val == 0;
        self.p.n = val & 0x80 != 0;
        self.p.v = val & 0x40 != 0;
    }

    fn bit_timed(&mut self, addr: u16, cycle_offset: u8, bus: &mut impl CPUBus) {
        let val = bus.cpu_read_timed(addr, cycle_offset);
        self.p.z = self.a & val == 0;
        self.p.n = val & 0x80 != 0;
        self.p.v = val & 0x40 != 0;
    }

    // unofficial
    fn ahx(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let tmp = addr - self.y as u16;
        if self.y as u16 + tmp < 0x00FF {
            let val = self.a & self.x & ((addr >> 8) as u8) + 1;
            bus.cpu_write(addr, val);
        } else {
            let val = bus.cpu_read(addr);
            bus.cpu_write(addr, val);
        }
    }

    fn alr(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.and(addr, bus);
        self.a = self.op_lsr(self.a);
    }

    fn anc(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.and(addr, bus);
        self.p.c = self.p.n;
    }

    fn axs(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let l = self.a & self.x;
        let r = bus.cpu_read(addr);
        self.x = l.wrapping_sub(r);
        self.set_zn(self.x);
        self.p.c = l >= r;
    }

    fn dcp(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let result = self.rmw_memory(addr, bus, |cpu, value| {
            let result = value.wrapping_sub(1);
            cpu.set_zn(result);
            result
        });
        self.cmp_value(self.a, result);
    }

    fn arr(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let f: u8 = if self.p.c { 0x80 } else { 0 };
        self.a = (bus.cpu_read(addr) & self.a) >> 1 | f;
        self.set_zn(self.a);
        self.p.c = self.a & 0x40 != 0;
        self.p.v = self.p.c != ((self.a & 0x20) != 0);
    }

    fn las(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let val = bus.cpu_read(addr);
        self.sp &= val;
        self.a = val;
        self.x = val;
        self.set_zn(val);
    }

    fn isc(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let result = self.rmw_memory(addr, bus, |cpu, value| {
            let result = value.wrapping_add(1);
            cpu.set_zn(result);
            result
        });
        self.sbc_value(result);
    }

    fn rla(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let result = self.rmw_memory(addr, bus, |cpu, value| cpu.op_rol(value));
        self.and_value(result);
    }

    fn rra(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let result = self.rmw_memory(addr, bus, |cpu, value| cpu.op_ror(value));
        self.adc_value(result);
    }

    fn lax(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let val = bus.cpu_read(addr);
        self.a = val;
        self.x = val;
        self.set_zn(val);
    }

    fn sax(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let val = self.a & self.x;
        bus.cpu_write(addr, val);
    }

    fn slo(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let result = self.rmw_memory(addr, bus, |cpu, value| cpu.op_asl(value));
        self.ora_value(result);
    }

    fn slo_timed(&mut self, addr: u16, mode: AddrMode, bus: &mut impl CPUBus) {
        let (read_cycle_offset, write_old_cycle_offset, write_new_cycle_offset) =
            Self::rmw_cycle_offsets(mode);
        let result = self.rmw_memory_timed(
            addr,
            bus,
            read_cycle_offset,
            write_old_cycle_offset,
            write_new_cycle_offset,
            |cpu, value| cpu.op_asl(value),
        );
        self.ora_value(result);
    }

    fn sre(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let result = self.rmw_memory(addr, bus, |cpu, value| cpu.op_lsr(value));
        self.eor_value(result);
    }

    fn tas(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.sp = self.a & self.x;
        let val = self.sp & (addr >> 8) as u8 + 1;
        let tmp = addr - self.y as u16;
        if self.y as u16 + tmp <= 0x00FF {
            bus.cpu_write(addr, val);
        } else {
            let val = bus.cpu_read(addr);
            bus.cpu_write(addr, val);
        }
    }

    fn shx(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let hi = ((addr >> 8) + 1) as u8;
        bus.cpu_write(addr, self.x & hi);
    }

    fn shy(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let hi = ((addr >> 8) + 1) as u8;
        bus.cpu_write(addr, self.y * hi);
    }
}

#[cfg(test)]
mod tests;
