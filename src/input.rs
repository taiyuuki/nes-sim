use crate::savestate::{SaveStateError, StateReader, StateWriter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerButton {
    A,
    B,
    Select,
    Start,
    Up,
    Down,
    Left,
    Right,
}

impl ControllerButton {
    const fn bit(self) -> u8 {
        match self {
            Self::A => 0x01,
            Self::B => 0x02,
            Self::Select => 0x04,
            Self::Start => 0x08,
            Self::Up => 0x10,
            Self::Down => 0x20,
            Self::Left => 0x40,
            Self::Right => 0x80,
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ControllerState {
    bits: u8,
}

impl ControllerState {
    pub const fn new() -> Self {
        Self { bits: 0 }
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    pub const fn bits(self) -> u8 {
        self.bits
    }

    pub const fn pressed(self, button: ControllerButton) -> bool {
        (self.bits & button.bit()) != 0
    }

    pub fn set_pressed(&mut self, button: ControllerButton, pressed: bool) {
        if pressed {
            self.bits |= button.bit();
        } else {
            self.bits &= !button.bit();
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Joypad {
    strobe: bool,
    state: ControllerState,
    shift: u8,
}

impl Joypad {
    pub(crate) const fn new() -> Self {
        Self {
            strobe: false,
            state: ControllerState::new(),
            shift: 0,
        }
    }

    pub(crate) fn set_state(&mut self, state: ControllerState) {
        self.state = state;
        if self.strobe {
            self.shift = state.bits();
        }
    }

    pub(crate) fn write(&mut self, data: u8) {
        self.strobe = (data & 0x01) != 0;
        if self.strobe {
            self.shift = self.state.bits();
        }
    }

    pub(crate) fn read(&mut self) -> u8 {
        if self.strobe {
            return u8::from(self.state.pressed(ControllerButton::A));
        }

        let bit = self.shift & 0x01;
        self.shift = (self.shift >> 1) | 0x80;
        bit
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.strobe);
        writer.write_u8(self.state.bits());
        writer.write_u8(self.shift);
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.strobe = reader.read_bool()?;
        self.state = ControllerState::from_bits(reader.read_u8()?);
        self.shift = reader.read_u8()?;
        Ok(())
    }
}
