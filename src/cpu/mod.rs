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
}

impl ResolvedOperand {
    fn new(addr: u16) -> Self {
        Self {
            addr,
            page_crossed: false,
        }
    }

    fn with_page_cross(addr: u16, page_crossed: bool) -> Self {
        Self { addr, page_crossed }
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
    XXX,
    ASO,
    NOP,
    ASL,
    PHP,
    BPL,
    CLC,
    NII,
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
    LSE,
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
    AXS,
    STY,
    STX,
    DEY,
    TXA,
    BCC,
    AXA,
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
    DCM,
    DEC,
    INY,
    DEX,
    BNE,
    CLD,
    CPX,
    SBC,
    INS,
    INC,
    INX,
    BEQ,
    SED,
}

#[derive(Clone, Copy)]
struct Inst(OpCode, AddrMode, u8); // opcode, addressing mode, cycles

const INST_SET: [Inst; 256] = [
    Inst(OpCode::BRK, AddrMode::IMP, 7),
    Inst(OpCode::ORA, AddrMode::IZX, 6),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::ASO, AddrMode::IZX, 8),
    Inst(OpCode::NOP, AddrMode::ZP0, 3),
    Inst(OpCode::ORA, AddrMode::ZP0, 3),
    Inst(OpCode::ASL, AddrMode::ZP0, 5),
    Inst(OpCode::ASO, AddrMode::ZP0, 5),
    Inst(OpCode::PHP, AddrMode::IMP, 3),
    Inst(OpCode::ORA, AddrMode::IMM, 2),
    Inst(OpCode::ASL, AddrMode::IMP, 2),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::NOP, AddrMode::ABS, 4),
    Inst(OpCode::ORA, AddrMode::ABS, 4),
    Inst(OpCode::ASL, AddrMode::ABS, 6),
    Inst(OpCode::ASO, AddrMode::ABS, 6),
    Inst(OpCode::BPL, AddrMode::REL, 2),
    Inst(OpCode::ORA, AddrMode::IZY, 5),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::ASO, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::ORA, AddrMode::ZPX, 4),
    Inst(OpCode::ASL, AddrMode::ZPX, 6),
    Inst(OpCode::ASO, AddrMode::ZPX, 6),
    Inst(OpCode::CLC, AddrMode::IMP, 2),
    Inst(OpCode::ORA, AddrMode::ABY, 4),
    Inst(OpCode::NII, AddrMode::IMP, 2),
    Inst(OpCode::ASO, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::ORA, AddrMode::ABX, 4),
    Inst(OpCode::ASL, AddrMode::ABX, 7),
    Inst(OpCode::ASO, AddrMode::ABX, 7),
    Inst(OpCode::JSR, AddrMode::ABS, 6),
    Inst(OpCode::AND, AddrMode::IZX, 6),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::RLA, AddrMode::IZX, 8),
    Inst(OpCode::BIT, AddrMode::ZP0, 3),
    Inst(OpCode::AND, AddrMode::ZP0, 3),
    Inst(OpCode::ROL, AddrMode::ZP0, 5),
    Inst(OpCode::RLA, AddrMode::ZP0, 5),
    Inst(OpCode::PLP, AddrMode::IMP, 4),
    Inst(OpCode::AND, AddrMode::IMM, 2),
    Inst(OpCode::ROL, AddrMode::IMP, 2),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::BIT, AddrMode::ABS, 4),
    Inst(OpCode::AND, AddrMode::ABS, 4),
    Inst(OpCode::ROL, AddrMode::ABS, 6),
    Inst(OpCode::RLA, AddrMode::ABS, 6),
    Inst(OpCode::BMI, AddrMode::REL, 2),
    Inst(OpCode::AND, AddrMode::IZY, 5),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::RLA, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::AND, AddrMode::ZPX, 4),
    Inst(OpCode::ROL, AddrMode::ZPX, 6),
    Inst(OpCode::RLA, AddrMode::ZPX, 6),
    Inst(OpCode::SEC, AddrMode::IMP, 2),
    Inst(OpCode::AND, AddrMode::ABY, 4),
    Inst(OpCode::NII, AddrMode::IMP, 2),
    Inst(OpCode::RLA, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::AND, AddrMode::ABX, 4),
    Inst(OpCode::ROL, AddrMode::ABX, 7),
    Inst(OpCode::RLA, AddrMode::ABX, 7),
    Inst(OpCode::RTI, AddrMode::IMP, 6),
    Inst(OpCode::EOR, AddrMode::IZX, 6),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::LSE, AddrMode::IZX, 8),
    Inst(OpCode::NOP, AddrMode::ZP0, 3),
    Inst(OpCode::EOR, AddrMode::ZP0, 3),
    Inst(OpCode::LSR, AddrMode::ZP0, 5),
    Inst(OpCode::LSE, AddrMode::ZP0, 5),
    Inst(OpCode::PHA, AddrMode::IMP, 3),
    Inst(OpCode::EOR, AddrMode::IMM, 2),
    Inst(OpCode::LSR, AddrMode::IMP, 2),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::JMP, AddrMode::ABS, 3),
    Inst(OpCode::EOR, AddrMode::ABS, 4),
    Inst(OpCode::LSR, AddrMode::ABS, 6),
    Inst(OpCode::LSE, AddrMode::ABS, 6),
    Inst(OpCode::BVC, AddrMode::REL, 2),
    Inst(OpCode::EOR, AddrMode::IZY, 5),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::LSE, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::EOR, AddrMode::ZPX, 4),
    Inst(OpCode::LSR, AddrMode::ZPX, 6),
    Inst(OpCode::LSE, AddrMode::ZPX, 6),
    Inst(OpCode::CLI, AddrMode::IMP, 2),
    Inst(OpCode::EOR, AddrMode::ABY, 4),
    Inst(OpCode::NII, AddrMode::IMP, 2),
    Inst(OpCode::LSE, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::EOR, AddrMode::ABX, 4),
    Inst(OpCode::LSR, AddrMode::ABX, 7),
    Inst(OpCode::LSE, AddrMode::ABX, 7),
    Inst(OpCode::RTS, AddrMode::IMP, 6),
    Inst(OpCode::ADC, AddrMode::IZX, 6),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::RRA, AddrMode::IZX, 8),
    Inst(OpCode::NOP, AddrMode::ZP0, 3),
    Inst(OpCode::ADC, AddrMode::ZP0, 3),
    Inst(OpCode::ROR, AddrMode::ZP0, 5),
    Inst(OpCode::RRA, AddrMode::ZP0, 5),
    Inst(OpCode::PLA, AddrMode::IMP, 4),
    Inst(OpCode::ADC, AddrMode::IMM, 2),
    Inst(OpCode::ROR, AddrMode::IMP, 2),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::JMP, AddrMode::IND, 5),
    Inst(OpCode::ADC, AddrMode::ABS, 4),
    Inst(OpCode::ROR, AddrMode::ABS, 6),
    Inst(OpCode::RRA, AddrMode::ABS, 6),
    Inst(OpCode::BVS, AddrMode::REL, 2),
    Inst(OpCode::ADC, AddrMode::IZY, 5),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::RRA, AddrMode::IZY, 8),
    Inst(OpCode::NOP, AddrMode::ZPX, 4),
    Inst(OpCode::ADC, AddrMode::ZPX, 4),
    Inst(OpCode::ROR, AddrMode::ZPX, 6),
    Inst(OpCode::RRA, AddrMode::ZPX, 6),
    Inst(OpCode::SEI, AddrMode::IMP, 2),
    Inst(OpCode::ADC, AddrMode::ABY, 4),
    Inst(OpCode::NII, AddrMode::IMP, 2),
    Inst(OpCode::RRA, AddrMode::ABY, 7),
    Inst(OpCode::NOP, AddrMode::ABX, 4),
    Inst(OpCode::ADC, AddrMode::ABX, 4),
    Inst(OpCode::ROR, AddrMode::ABX, 7),
    Inst(OpCode::RRA, AddrMode::ABX, 7),
    Inst(OpCode::NII, AddrMode::IMM, 2),
    Inst(OpCode::STA, AddrMode::IZX, 6),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::AXS, AddrMode::IZX, 6),
    Inst(OpCode::STY, AddrMode::ZP0, 3),
    Inst(OpCode::STA, AddrMode::ZP0, 3),
    Inst(OpCode::STX, AddrMode::ZP0, 3),
    Inst(OpCode::AXS, AddrMode::ZP0, 3),
    Inst(OpCode::DEY, AddrMode::IMP, 2),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::TXA, AddrMode::IMP, 2),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::STY, AddrMode::ABS, 4),
    Inst(OpCode::STA, AddrMode::ABS, 4),
    Inst(OpCode::STX, AddrMode::ABS, 4),
    Inst(OpCode::AXS, AddrMode::ABS, 4),
    Inst(OpCode::BCC, AddrMode::REL, 2),
    Inst(OpCode::STA, AddrMode::IZY, 6),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::AXA, AddrMode::IZY, 6),
    Inst(OpCode::STY, AddrMode::ZPX, 4),
    Inst(OpCode::STA, AddrMode::ZPX, 4),
    Inst(OpCode::STX, AddrMode::ZPY, 4),
    Inst(OpCode::AXS, AddrMode::ZPY, 4),
    Inst(OpCode::TYA, AddrMode::IMP, 2),
    Inst(OpCode::STA, AddrMode::ABY, 5),
    Inst(OpCode::TXS, AddrMode::IMP, 2),
    Inst(OpCode::AXA, AddrMode::ABY, 5),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::STA, AddrMode::ABX, 5),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
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
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::LDY, AddrMode::ABS, 4),
    Inst(OpCode::LDA, AddrMode::ABS, 4),
    Inst(OpCode::LDX, AddrMode::ABS, 4),
    Inst(OpCode::LAX, AddrMode::ABS, 4),
    Inst(OpCode::BCS, AddrMode::REL, 2),
    Inst(OpCode::LDA, AddrMode::IZY, 5),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::LAX, AddrMode::IZY, 5),
    Inst(OpCode::LDY, AddrMode::ZPX, 4),
    Inst(OpCode::LDA, AddrMode::ZPX, 4),
    Inst(OpCode::LDX, AddrMode::ZPY, 4),
    Inst(OpCode::LAX, AddrMode::ZPY, 4),
    Inst(OpCode::CLV, AddrMode::IMP, 2),
    Inst(OpCode::LDA, AddrMode::ABY, 4),
    Inst(OpCode::TSX, AddrMode::IMP, 2),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::LDY, AddrMode::ABX, 4),
    Inst(OpCode::LDA, AddrMode::ABX, 4),
    Inst(OpCode::LDX, AddrMode::ABY, 4),
    Inst(OpCode::LAX, AddrMode::ABY, 4),
    Inst(OpCode::CPY, AddrMode::IMM, 2),
    Inst(OpCode::CMP, AddrMode::IZX, 6),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::DCM, AddrMode::IZX, 8),
    Inst(OpCode::CPY, AddrMode::ZP0, 3),
    Inst(OpCode::CMP, AddrMode::ZP0, 3),
    Inst(OpCode::DEC, AddrMode::ZP0, 5),
    Inst(OpCode::DCM, AddrMode::ZP0, 5),
    Inst(OpCode::INY, AddrMode::IMP, 2),
    Inst(OpCode::CMP, AddrMode::IMM, 2),
    Inst(OpCode::DEX, AddrMode::IMP, 2),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::CPY, AddrMode::ABS, 4),
    Inst(OpCode::CMP, AddrMode::ABS, 4),
    Inst(OpCode::DEC, AddrMode::ABS, 6),
    Inst(OpCode::DCM, AddrMode::ABS, 6),
    Inst(OpCode::BNE, AddrMode::REL, 2),
    Inst(OpCode::CMP, AddrMode::IZY, 5),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::DCM, AddrMode::IZY, 8),
    Inst(OpCode::NII, AddrMode::ZPX, 4),
    Inst(OpCode::CMP, AddrMode::ZPX, 4),
    Inst(OpCode::DEC, AddrMode::ZPX, 6),
    Inst(OpCode::DCM, AddrMode::ZPX, 6),
    Inst(OpCode::CLD, AddrMode::IMP, 2),
    Inst(OpCode::CMP, AddrMode::ABY, 4),
    Inst(OpCode::NII, AddrMode::IMP, 2),
    Inst(OpCode::DCM, AddrMode::ABY, 7),
    Inst(OpCode::NII, AddrMode::ABX, 4),
    Inst(OpCode::CMP, AddrMode::ABX, 4),
    Inst(OpCode::DEC, AddrMode::ABX, 7),
    Inst(OpCode::DCM, AddrMode::ABX, 7),
    Inst(OpCode::CPX, AddrMode::IMM, 2),
    Inst(OpCode::SBC, AddrMode::IZX, 6),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::INS, AddrMode::IZX, 8),
    Inst(OpCode::CPX, AddrMode::ZP0, 3),
    Inst(OpCode::SBC, AddrMode::ZP0, 3),
    Inst(OpCode::INC, AddrMode::ZP0, 5),
    Inst(OpCode::INS, AddrMode::ZP0, 5),
    Inst(OpCode::INX, AddrMode::IMP, 2),
    Inst(OpCode::SBC, AddrMode::IMM, 2),
    Inst(OpCode::NII, AddrMode::IMP, 2),
    Inst(OpCode::SBC, AddrMode::IMM, 2),
    Inst(OpCode::CPX, AddrMode::ABS, 4),
    Inst(OpCode::SBC, AddrMode::ABS, 4),
    Inst(OpCode::INC, AddrMode::ABS, 6),
    Inst(OpCode::INS, AddrMode::ABS, 6),
    Inst(OpCode::BEQ, AddrMode::REL, 2),
    Inst(OpCode::SBC, AddrMode::IZY, 5),
    Inst(OpCode::XXX, AddrMode::IMP, 2),
    Inst(OpCode::INS, AddrMode::IZY, 8),
    Inst(OpCode::NII, AddrMode::ZPX, 4),
    Inst(OpCode::SBC, AddrMode::ZPX, 4),
    Inst(OpCode::INC, AddrMode::ZPX, 6),
    Inst(OpCode::INS, AddrMode::ZPX, 6),
    Inst(OpCode::SED, AddrMode::IMP, 2),
    Inst(OpCode::SBC, AddrMode::ABY, 4),
    Inst(OpCode::NII, AddrMode::IMP, 2),
    Inst(OpCode::INS, AddrMode::ABY, 7),
    Inst(OpCode::NII, AddrMode::ABX, 4),
    Inst(OpCode::SBC, AddrMode::ABX, 4),
    Inst(OpCode::INC, AddrMode::ABX, 7),
    Inst(OpCode::INS, AddrMode::ABX, 7),
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
                Some(ResolvedOperand::with_page_cross(
                    addr,
                    Self::page_crossed(base, addr),
                ))
            }
            AddrMode::ABY => {
                let base = self.fetch_u16(bus);
                let addr = base.wrapping_add(self.y as u16);
                Some(ResolvedOperand::with_page_cross(
                    addr,
                    Self::page_crossed(base, addr),
                ))
            }
            AddrMode::IND => Some(ResolvedOperand::new(self.ind(bus))),
            AddrMode::IZX => Some(ResolvedOperand::new(self.izx(bus))),
            AddrMode::IZY => {
                let ptr = self.fetch_byte(bus);
                let base = self.read_u16_zero_page(ptr, bus);
                let addr = base.wrapping_add(self.y as u16);
                Some(ResolvedOperand::with_page_cross(
                    addr,
                    Self::page_crossed(base, addr),
                ))
            }
        }
    }

    fn require_operand(&mut self, mode: AddrMode, bus: &mut impl CPUBus) -> ResolvedOperand {
        self.resolve_operand(mode, bus)
            .expect("Instruction requires an operand address")
    }

    // fn fetch_operand(&mut self, addr: u16, bus: &mut impl CPUBus) -> u8 {
    //     self.resolve_operand_addr(mode, bus)
    //         .map(|addr| bus.cpu_read(addr))
    //         .unwrap_or(0)
    // }

    pub fn exe_inst(&mut self, inst_byte: u8, bus: &mut impl CPUBus) {
        let inst = INST_SET[inst_byte as usize];
        let mut extra_cycles = 0_u64;
        match inst.0 {
            // Do nothing.
            OpCode::NOP => {
                let _ = self.resolve_operand(inst.1, bus);
            }
            // Bitwise
            OpCode::ORA => {
                let operand = self.require_operand(inst.1, bus);
                self.ora(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::AND => {
                let operand = self.require_operand(inst.1, bus);
                self.and(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::BIT => {
                let operand = self.require_operand(inst.1, bus);
                self.bit(operand.addr, bus);
            }
            // Shift
            OpCode::EOR => {
                let operand = self.require_operand(inst.1, bus);
                self.eor(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::ASL => {
                if matches!(inst.1, AddrMode::IMP) {
                    self.a = self.op_asl(self.a);
                } else {
                    let operand = self.require_operand(inst.1, bus);
                    self.asl(operand.addr, bus);
                }
            }
            OpCode::ROL => {
                if matches!(inst.1, AddrMode::IMP) {
                    self.a = self.op_rol(self.a);
                } else {
                    let operand = self.require_operand(inst.1, bus);
                    self.rol(operand.addr, bus);
                }
            }
            OpCode::LSR => {
                if matches!(inst.1, AddrMode::IMP) {
                    self.a = self.op_lsr(self.a);
                } else {
                    let operand = self.require_operand(inst.1, bus);
                    self.lsr(operand.addr, bus);
                }
            }
            OpCode::ROR => {
                if matches!(inst.1, AddrMode::IMP) {
                    self.a = self.op_ror(self.a);
                } else {
                    let operand = self.require_operand(inst.1, bus);
                    self.ror(operand.addr, bus);
                }
            }
            // Access
            OpCode::LDA => {
                let operand = self.require_operand(inst.1, bus);
                self.lda(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::LDX => {
                let operand = self.require_operand(inst.1, bus);
                self.ldx(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::LDY => {
                let operand = self.require_operand(inst.1, bus);
                self.ldy(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::STA => {
                let operand = self.require_operand(inst.1, bus);
                self.sta(operand.addr, bus);
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
                self.inc(operand.addr, bus);
            }
            OpCode::DEC => {
                let operand = self.require_operand(inst.1, bus);
                self.dec(operand.addr, bus);
            }
            OpCode::ADC => {
                let operand = self.require_operand(inst.1, bus);
                self.adc(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            OpCode::SBC => {
                let operand = self.require_operand(inst.1, bus);
                self.sbc(operand.addr, bus);
                extra_cycles += inst.1.read_page_cross_penalty(operand);
            }
            _ => {
                let _ = self.resolve_operand(inst.1, bus);
            } // Illegal opcode, do nothing
        }
        self.cycles += inst.2 as u64 + extra_cycles;
    }

    pub fn cpu_clock(&mut self, bus: &mut impl CPUBus) {
        bus.cpu_read(0x4000); // 同步APU，暂时无用，后续可能要移动到BUS或NES
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

    fn nmi_interrupt(&mut self, bus: &mut impl CPUBus) {
        self.stack_push((self.pc >> 8) as u8, bus);
        self.stack_push(self.pc as u8, bus);
        let p = self.status_byte_for_push(true) & !0x10;
        self.stack_push(p, bus);
        self.p.i = true;
        self.pc = bus.cpu_read_u16(0xFFFA);
    }

    fn irq_interrupt(&mut self, bus: &mut impl CPUBus) {
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
        self.a |= bus.cpu_read(addr);
        self.set_zn(self.a);
    }

    fn and(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.a &= bus.cpu_read(addr);
        self.set_zn(self.a);
    }

    fn eor(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.a ^= bus.cpu_read(addr);
        self.set_zn(self.a);
    }

    fn lda(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.a = bus.cpu_read(addr);
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
        let val = bus.cpu_read(addr).wrapping_add(1);
        bus.cpu_write(addr, val);
        self.set_zn(val);
    }

    fn dec(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let val = bus.cpu_read(addr).wrapping_sub(1);
        bus.cpu_write(addr, val);
        self.set_zn(val);
    }

    // 直接跳到目标地址
    fn jmp(&mut self, addr: u16) {
        self.pc = addr;
    }

    // 跳到目标地址，但要先将当前地址压入栈，栈方向是向下的，所以要先减一
    fn jsr(&mut self, addr: u16, bus: &mut impl CPUBus) {
        self.pc = self.pc.wrapping_sub(1);
        self.stack_push((self.pc >> 8 & 0xFF) as u8, bus);
        self.stack_push((self.pc & 0xFF) as u8, bus);
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
        let data = bus.cpu_read(addr);
        self.cmp_core(self.a, data);
    }

    fn cpx(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let data = bus.cpu_read(addr);
        self.cmp_core(self.x, data);
    }

    fn cpy(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let data = bus.cpu_read(addr);
        self.cmp_core(self.y, data);
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
        let val = bus.cpu_read(addr);
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
        let val = bus.cpu_read(addr) ^ 0xFF;
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
        let val = bus.cpu_read(addr);
        let result = self.op_asl(val);
        bus.cpu_write(addr, result);
    }

    fn lsr(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let val = bus.cpu_read(addr);
        let result = self.op_lsr(val);
        bus.cpu_write(addr, result);
    }

    fn rol(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let val = bus.cpu_read(addr);
        let result = self.op_rol(val);
        bus.cpu_write(addr, result);
    }

    fn ror(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let val = bus.cpu_read(addr);
        let result = self.op_ror(val);
        bus.cpu_write(addr, result);
    }

    fn bit(&mut self, addr: u16, bus: &mut impl CPUBus) {
        let val = bus.cpu_read(addr);
        self.p.z = self.a & val == 0;
        self.p.n = val & 0x80 != 0;
        self.p.v = val & 0x40 != 0;
    }
}

#[cfg(test)]
mod tests;
