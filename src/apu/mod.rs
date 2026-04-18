pub enum DmcDmaKind {
    Load,
    Reload,
}

pub struct DmcDmaRequest {
    pub addr: u16,
    pub kind: DmcDmaKind,
}

pub struct APU {}

impl APU {
    pub fn new() -> Self {
        Self {}
    }

    pub fn reset(&self) {}

    pub fn save_state() {}
}
