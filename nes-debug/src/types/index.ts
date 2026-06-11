export interface CpuInfo {
    a:                   number;
    x:                   number;
    y:                   number;
    sp:                  number;
    pc:                  number;
    status:              number;
    clocks:              number;
    instruction_counter: number;
    irq_pending:         boolean;
    nmi_line:            boolean;
}

export interface PpuInfo {
    frame:          number;
    scanline:       number;
    in_vblank:      boolean;
    nmi_line:       boolean;
    oam_addr:       number;
    cycles:         number;
    ctrl:           number;
    mask:           number;
    status:         number;
    vram_addr:      number;
    temp_vram_addr: number;
    bg_on:          boolean;
    sprites_on:     boolean;
    rendering_on:   boolean;
}

export interface DebugInfo {
    master_clock: number;
    cpu:          CpuInfo;
    ppu:          PpuInfo;
    paused:       boolean;
    frame_number: number;
}

export interface FrameData {
    width:      number;
    height:     number;
    pixels_b64: string;
}

export interface RunFrameResult {
    debug_info: DebugInfo;
    frame:      FrameData;
}

export interface BreakpointDef {
    type:   'address' | 'memory_read' | 'memory_write' | 'ppu_scanline' | 'vblank';
    value?: number;
}

export interface DisasmInstruction {
    address:  number;
    bytes:    [number, number, number];
    len:      number;
    mnemonic: string;
    operand:  string;
}

export interface DisasmResult {
    instructions: DisasmInstruction[];
    pc_index:     number;
}

export interface PatternTableData {
    table0_b64: string;
    table1_b64: string;
    size:       number;
}

export interface NametableData {
    pixels_b64: string;
    width:      number;
    height:     number;
}

export interface AudioData {
    samples:     number[];
    sample_rate: number;
}
