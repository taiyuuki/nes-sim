use crate::apu::dmc::{DmcDmaKind, DmcDmaRequest};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

/// DMA控制器在一个CPU周期内需要总线执行的操作
pub enum DmaBusRequest {
    /// 无需总线操作
    None,
    /// DMC DMA读取：从addr读取一字节，作为DMC采样提交
    DmcRead { addr: u16 },
    /// OAM DMA读取：从addr读取一字节
    OamRead { addr: u16 },
    /// OAM DMA写入：将data写入OAM
    OamWrite { data: u8 },
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DmcDmaState {
    AwaitHalt,
    Dummy,
    Align,
    Read,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DmcDma {
    addr: u16,
    halt_phase: CpuSlotPhase,
    state: DmcDmaState,
}

impl DmcDma {
    fn new(request: DmcDmaRequest) -> Self {
        Self {
            addr: request.addr,
            halt_phase: match request.kind {
                DmcDmaKind::Load => CpuSlotPhase::Get,
                DmcDmaKind::Reload => CpuSlotPhase::Put,
            },
            state: DmcDmaState::AwaitHalt,
        }
    }
}

pub struct DmaController {
    pending_oam: Option<u8>,
    active_oam: Option<OamDma>,
    active_dmc: Option<DmcDma>,
    cpu_phase: CpuSlotPhase,
}

impl DmaController {
    pub fn new() -> Self {
        Self {
            pending_oam: None,
            active_oam: None,
            active_dmc: None,
            cpu_phase: CpuSlotPhase::Get,
        }
    }

    pub fn request_oam_dma(&mut self, page: u8) {
        self.pending_oam = Some(page);
    }

    pub fn in_progress(&self) -> bool {
        self.pending_oam.is_some() || self.active_oam.is_some() || self.active_dmc.is_some()
    }

    pub fn tick_cpu_cycle(&mut self, dmc_request: Option<DmcDmaRequest>) -> (bool, DmaBusRequest) {
        if self.active_dmc.is_none() {
            if let Some(request) = dmc_request {
                self.active_dmc = Some(DmcDma::new(request));
            }
        }

        if let Some(dma) = self.active_dmc.as_mut() {
            match dma.state {
                DmcDmaState::AwaitHalt => {
                    if self.cpu_phase == dma.halt_phase {
                        dma.state = DmcDmaState::Dummy;
                    }
                }
                DmcDmaState::Dummy => {
                    dma.state = if self.cpu_phase == CpuSlotPhase::Put {
                        DmcDmaState::Read
                    } else {
                        DmcDmaState::Align
                    };
                }
                DmcDmaState::Align => {
                    dma.state = DmcDmaState::Read;
                }
                DmcDmaState::Read => {
                    return (true, DmaBusRequest::DmcRead { addr: dma.addr });
                }
            }
            return (true, DmaBusRequest::None);
        }

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
                    return (
                        true,
                        DmaBusRequest::OamRead {
                            addr: dma.source_addr(),
                        },
                    );
                }
                OamDmaState::Write => {
                    return (true, DmaBusRequest::OamWrite { data: dma.latch });
                }
            }
        }
        (consumed, DmaBusRequest::None)
    }

    pub fn apply_dmc_read(&mut self) {
        self.active_dmc = None;
    }

    pub fn apply_oam_read(&mut self, data: u8) {
        if let Some(dma) = self.active_oam.as_mut() {
            dma.latch = data;
            dma.state = OamDmaState::Write;
        }
    }

    pub fn apply_oam_write(&mut self) {
        if let Some(dma) = self.active_oam.as_mut() {
            dma.index = dma.index.wrapping_add(1);
            if dma.index == 0 {
                self.active_oam = None;
            } else {
                dma.state = OamDmaState::Read;
            }
        }
    }

    pub fn advance_cpu_phase(&mut self) {
        self.cpu_phase = self.cpu_phase.toggle();
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        match self.pending_oam {
            Some(page) => {
                writer.write_bool(true);
                writer.write_u8(page);
            }
            None => writer.write_bool(false),
        }

        match self.active_oam {
            Some(dma) => {
                writer.write_bool(true);
                writer.write_u8(dma.page);
                writer.write_u8(dma.index);
                writer.write_u8(dma.latch);
                writer.write_u8(match dma.state {
                    OamDmaState::Halt => 0,
                    OamDmaState::Align => 1,
                    OamDmaState::Read => 2,
                    OamDmaState::Write => 3,
                });
            }
            None => writer.write_bool(false),
        }

        match self.active_dmc {
            Some(dma) => {
                writer.write_bool(true);
                writer.write_u16(dma.addr);
                writer.write_u8(match dma.halt_phase {
                    CpuSlotPhase::Get => 0,
                    CpuSlotPhase::Put => 1,
                });
                writer.write_u8(match dma.state {
                    DmcDmaState::AwaitHalt => 0,
                    DmcDmaState::Dummy => 1,
                    DmcDmaState::Align => 2,
                    DmcDmaState::Read => 3,
                });
            }
            None => writer.write_bool(false),
        }

        writer.write_u8(match self.cpu_phase {
            CpuSlotPhase::Get => 0,
            CpuSlotPhase::Put => 1,
        });
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.pending_oam = if reader.read_bool()? {
            Some(reader.read_u8()?)
        } else {
            None
        };

        self.active_oam = if reader.read_bool()? {
            let page = reader.read_u8()?;
            let index = reader.read_u8()?;
            let latch = reader.read_u8()?;
            let state = match reader.read_u8()? {
                0 => OamDmaState::Halt,
                1 => OamDmaState::Align,
                2 => OamDmaState::Read,
                3 => OamDmaState::Write,
                _ => return Err(SaveStateError::InvalidData("invalid OAM DMA state")),
            };
            Some(OamDma {
                page,
                index,
                latch,
                state,
            })
        } else {
            None
        };

        self.active_dmc = if reader.read_bool()? {
            let addr = reader.read_u16()?;
            let halt_phase = match reader.read_u8()? {
                0 => CpuSlotPhase::Get,
                1 => CpuSlotPhase::Put,
                _ => return Err(SaveStateError::InvalidData("invalid DMC DMA halt phase")),
            };
            let state = match reader.read_u8()? {
                0 => DmcDmaState::AwaitHalt,
                1 => DmcDmaState::Dummy,
                2 => DmcDmaState::Align,
                3 => DmcDmaState::Read,
                _ => return Err(SaveStateError::InvalidData("invalid DMC DMA state")),
            };
            Some(DmcDma {
                addr,
                halt_phase,
                state,
            })
        } else {
            None
        };

        self.cpu_phase = match reader.read_u8()? {
            0 => CpuSlotPhase::Get,
            1 => CpuSlotPhase::Put,
            _ => return Err(SaveStateError::InvalidData("invalid DMA CPU phase")),
        };
        Ok(())
    }
}

impl Default for DmaController {
    fn default() -> Self {
        Self::new()
    }
}
