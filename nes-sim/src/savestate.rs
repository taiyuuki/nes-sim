use std::error::Error;
use std::fmt::{Display, Formatter};

const SAVE_STATE_MAGIC: &[u8; 8] = b"NESSTAT\0";
const SAVE_STATE_VERSION: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum SaveStateError {
    InvalidMagic,
    UnsupportedVersion(u32),
    UnexpectedEof,
    TrailingData,
    NoCartridge,
    MapperMismatch { expected: u16, actual: u16 },
    InvalidData(&'static str),
}

impl Display for SaveStateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMagic => f.write_str("save state header is invalid"),
            Self::UnsupportedVersion(version) => {
                write!(f, "save state version {} is not supported", version)
            }
            Self::UnexpectedEof => f.write_str("save state ended unexpectedly"),
            Self::TrailingData => f.write_str("save state contains trailing bytes"),
            Self::NoCartridge => f.write_str("save state requires an inserted cartridge"),
            Self::MapperMismatch { expected, actual } => write!(
                f,
                "save state mapper mismatch: expected mapper {}, got mapper {}",
                expected, actual
            ),
            Self::InvalidData(message) => f.write_str(message),
        }
    }
}

impl Error for SaveStateError {}

pub(crate) struct StateWriter {
    bytes: Vec<u8>,
}

impl StateWriter {
    pub(crate) fn new() -> Self {
        let mut writer = Self { bytes: Vec::new() };
        writer.write_bytes(SAVE_STATE_MAGIC);
        writer.write_u32(SAVE_STATE_VERSION);
        writer
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }

    pub(crate) fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn write_bool(&mut self, value: bool) {
        self.write_u8(u8::from(value));
    }

    pub(crate) fn write_u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_i16(&mut self, value: i16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }
}

pub(crate) struct StateReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> StateReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Result<Self, SaveStateError> {
        let mut reader = Self { bytes, offset: 0 };
        let mut magic = [0; SAVE_STATE_MAGIC.len()];
        reader.read_exact(&mut magic)?;
        if &magic != SAVE_STATE_MAGIC {
            return Err(SaveStateError::InvalidMagic);
        }

        let version = reader.read_u32()?;
        if version != SAVE_STATE_VERSION {
            return Err(SaveStateError::UnsupportedVersion(version));
        }

        Ok(reader)
    }

    pub(crate) fn finish(self) -> Result<(), SaveStateError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(SaveStateError::TrailingData)
        }
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, SaveStateError> {
        let mut buf = [0];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    pub(crate) fn read_bool(&mut self) -> Result<bool, SaveStateError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SaveStateError::InvalidData("boolean field must be 0 or 1")),
        }
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, SaveStateError> {
        let mut buf = [0; 2];
        self.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, SaveStateError> {
        let mut buf = [0; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64, SaveStateError> {
        let mut buf = [0; 8];
        self.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    pub(crate) fn read_i16(&mut self) -> Result<i16, SaveStateError> {
        let mut buf = [0; 2];
        self.read_exact(&mut buf)?;
        Ok(i16::from_le_bytes(buf))
    }

    pub(crate) fn read_bytes_into(&mut self, bytes: &mut [u8]) -> Result<(), SaveStateError> {
        self.read_exact(bytes)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), SaveStateError> {
        let end = self.offset.saturating_add(buf.len());
        if end > self.bytes.len() {
            return Err(SaveStateError::UnexpectedEof);
        }
        buf.copy_from_slice(&self.bytes[self.offset..end]);
        self.offset = end;
        Ok(())
    }
}
