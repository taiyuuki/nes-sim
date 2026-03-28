use crate::bus::NESBus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CpuSlotPhase {
    Get,
    Put,
}

impl CpuSlotPhase {
    fn toggle(self) -> Self {
        match self {
            Self::Get => Self::Put,
            Self::Put => Self::Get,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OamDmaState {
    Halt,
    Align,
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OamDma {
    page: u8,
    index: u8,
    latch: u8,
    state: OamDmaState,
}

impl OamDma {
    fn new(page: u8) -> Self {
        Self {
            page,
            index: 0,
            latch: 0,
            state: OamDmaState::Halt,
        }
    }

    fn source_addr(self) -> u16 {
        ((self.page as u16) << 8) | self.index as u16
    }
}

pub struct DmaController {
    pending_oam: Option<u8>,
    active_oam: Option<OamDma>,
    cpu_phase: CpuSlotPhase,
}

impl DmaController {
    pub fn new() -> Self {
        Self {
            pending_oam: None,
            active_oam: None,
            cpu_phase: CpuSlotPhase::Get,
        }
    }

    pub fn request_oam_dma(&mut self, page: u8) {
        self.pending_oam = Some(page);
    }

    pub fn in_progress(&self) -> bool {
        self.pending_oam.is_some() || self.active_oam.is_some()
    }

    pub fn tick_cpu_cycle(&mut self, bus: &mut NESBus) -> bool {
        if self.active_oam.is_none() {
            if let Some(page) = self.pending_oam.take() {
                self.active_oam = Some(OamDma::new(page));
            }
        }

        let mut consumed = false;

        if let Some(dma) = self.active_oam.as_mut() {
            consumed = true;
            match dma.state {
                OamDmaState::Halt => {
                    dma.state = match self.cpu_phase {
                        CpuSlotPhase::Get => OamDmaState::Align,
                        CpuSlotPhase::Put => OamDmaState::Read,
                    };
                }
                OamDmaState::Align => {
                    dma.state = OamDmaState::Read;
                }
                OamDmaState::Read => {
                    dma.latch = bus.dma_read(dma.source_addr());
                    dma.state = OamDmaState::Write;
                }
                OamDmaState::Write => {
                    bus.dma_write_oam(dma.latch);
                    dma.index = dma.index.wrapping_add(1);
                    if dma.index == 0 {
                        self.active_oam = None;
                    } else {
                        dma.state = OamDmaState::Read;
                    }
                }
            }
        }

        self.cpu_phase = self.cpu_phase.toggle();
        consumed
    }
}

impl Default for DmaController {
    fn default() -> Self {
        Self::new()
    }
}
