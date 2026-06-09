use std::error::Error;
use std::fmt::{Display, Formatter};

pub(crate) mod expansion_audio;
mod mappers;

use self::mappers::{Mapper, from_mapper_id};
use crate::apu::ExpansionAudioChip;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const INES_HEADER_LEN: usize = 16;
const TRAINER_LEN: usize = 512;
const PRG_BANK_LEN: usize = 0x4000;
const CHR_BANK_LEN: usize = 0x2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SPAGE0,
    SPAGE1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TVSystem {
    NTSC,
    PAL,
    DENDY,
}

pub enum TimingMode {
    NTSC,
    Pal,
    MultipleRegion,
    Dendy,
}

impl TimingMode {
    fn decode_ines(flag9: u8, flag10: u8) -> Self {
        match flag10 & 0x03 {
            0x02 => Self::Pal,
            0x03 => Self::MultipleRegion,
            _ => {
                if (flag9 & 0x01) != 0 {
                    Self::Pal
                } else {
                    Self::NTSC
                }
            }
        }
    }

    fn decode_nes20(encoded: u8) -> Self {
        match encoded {
            1 => Self::Pal,
            2 => Self::MultipleRegion,
            3 => Self::Dendy,
            _ => Self::NTSC,
        }
    }

    fn to_tv_system(&self) -> TVSystem {
        match self {
            TimingMode::Pal => TVSystem::PAL,
            TimingMode::Dendy => TVSystem::DENDY,
            TimingMode::NTSC | TimingMode::MultipleRegion => TVSystem::NTSC,
        }
    }
}

pub enum RomFormat {
    INES,
    NES20,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CartridgeError {
    FileTooSmall,
    InvalidMagic,
    Nes2Unsupported,
    UnsupportedMapper(u16),
    TruncatedData,
}

impl Display for CartridgeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileTooSmall => f.write_str("ROM is smaller than the 16-byte iNES header"),
            Self::InvalidMagic => f.write_str("ROM is not in iNES format"),
            Self::Nes2Unsupported => f.write_str("NES 2.0 ROMs are not supported yet"),
            Self::UnsupportedMapper(id) => write!(f, "mapper {} is not supported yet", id),
            Self::TruncatedData => f.write_str("ROM ended before PRG/CHR data was fully present"),
        }
    }
}

impl Error for CartridgeError {}

fn decode_nes20_rom_size(lsb: u16, msb_nibble: u16, unit_size: u16) -> u32 {
    if msb_nibble != 0x0F {
        return ((msb_nibble << 8) | lsb) as u32 * (unit_size as u32);
    }

    let multiplier = (lsb & 0x03) * 2 + 1;
    let exponent = lsb >> 2;
    (multiplier as u32) << exponent
}

fn decode_nes20_ram_size(encode_shift_count: u8) -> u32 {
    if encode_shift_count == 0 {
        0
    } else {
        64 << encode_shift_count
    }
}

fn decode_ines_prg_ram_size(encoded_units: u8) -> u16 {
    let unit = if encoded_units == 0 { 1 } else { encoded_units };
    unit as u16 * 0x2000
}

fn decode_mirroring(flags6: u8) -> Mirroring {
    if flags6 & 0x08 != 0 {
        return Mirroring::FourScreen;
    }
    if flags6 & 0x01 != 0 {
        return Mirroring::Vertical;
    }
    Mirroring::Horizontal
}

#[allow(dead_code)]
struct CartridgeHeader {
    raw: Vec<u8>,
    format: RomFormat,
    mapper_id: u16,
    submapper: u8,
    mirroring: Mirroring,
    has_battery_backed_ram: bool,
    has_trainer: bool,
    has_bus_conflicts: bool,
    console_type: u8,
    console_type_data: u8,
    timing_mode: TimingMode,
    tv_system: TVSystem,
    prg_rom_size: u32,
    prg_ram_size: u32,
    prg_nvram_size: u32,
    chr_rom_size: u32,
    chr_ram_size: u32,
    chr_nvram_size: u32,
    misc_rom_count: u8,
    defaut_expansion_device: u8,
    has_prg_ram_info: bool,
    uses_exponent_rom_size_encoding: bool,
}

impl CartridgeHeader {
    fn parse(rom: &[u8]) -> Result<CartridgeHeader, CartridgeError> {
        if rom.len() < INES_HEADER_LEN {
            return Err(CartridgeError::FileTooSmall);
        }

        if &rom[0..4] != b"NES\x1A" {
            return Err(CartridgeError::FileTooSmall);
        }

        let total_len = rom.len();
        let raw = rom[0..INES_HEADER_LEN].to_vec();
        let flags6 = raw[6] as u16;
        let flags7 = raw[7] as u16;
        let flags8 = raw[8] as u16;
        let has_trainer = (flags6 & 0x04) != 0;

        let mut format = if (raw[7] & 0x0C) != 0x08 {
            RomFormat::INES
        } else {
            RomFormat::NES20
        };

        let prg_rom_size =
            decode_nes20_rom_size(raw[4] as u16, (raw[9] as u16) & 0x0F, PRG_BANK_LEN as u16);
        let chr_rom_size =
            decode_nes20_rom_size(raw[5] as u16, (raw[9] as u16) >> 4, PRG_BANK_LEN as u16);

        let trainer_size = if (raw[6] & 0x04) != 0 { total_len } else { 0 };
        let required_bytes =
            INES_HEADER_LEN + trainer_size + (prg_rom_size + chr_rom_size) as usize;

        if required_bytes <= total_len {
            format = RomFormat::INES;
        }

        let mirroring = decode_mirroring(raw[6]);
        let has_sram = flags6 & 0x02 == 0;
        let console_type = raw[7] & 0x03;
        let console_type_data = raw[13];

        return match format {
            RomFormat::NES20 => {
                let timing_mode = TimingMode::decode_nes20(raw[12] & 0x03);
                let mapper_id = (flags6 >> 4) | (flags7 & 0xF0) | ((flags8 & 0x0F) << 8);
                let submapper = (flags8 >> 4) as u8;
                let tv_system = timing_mode.to_tv_system();
                let prg_ram_size = decode_nes20_ram_size(raw[10] & 0x0F);
                let prg_nvram_size = decode_nes20_ram_size(raw[10] << 4);
                let chr_ram_size = decode_nes20_ram_size(raw[11] & 0x0F);
                let chr_nvram_size = decode_nes20_ram_size(raw[11] >> 4);
                let misc_rom_count = raw[14] & 0x03;
                let defaut_expansion_device = raw[15] & 0x3F;
                let uses_exponent_rom_size_encoding =
                    ((raw[9] & 0x0F) == 0x0F) || ((raw[9] >> 4) == 0x0F);

                Ok(CartridgeHeader {
                    raw,
                    format,
                    mapper_id,
                    submapper,
                    mirroring,
                    has_battery_backed_ram: has_sram,
                    has_trainer,
                    has_bus_conflicts: false,
                    console_type,
                    console_type_data,
                    timing_mode,
                    tv_system,
                    prg_rom_size,
                    chr_rom_size,
                    prg_ram_size,
                    prg_nvram_size,
                    chr_ram_size,
                    chr_nvram_size,
                    misc_rom_count,
                    defaut_expansion_device,
                    has_prg_ram_info: true,
                    uses_exponent_rom_size_encoding,
                })
            }
            RomFormat::INES => {
                let has_trusted_ines_extension = raw[12..INES_HEADER_LEN].iter().all(|b| *b == 0);
                let mapper_id = if has_sram {
                    (flags6 >> 4) | (flags7 & 0xF0)
                } else {
                    flags6 >> 4
                };

                let prg_rom_size = (raw[4] as u32) * (PRG_BANK_LEN as u32);
                let chr_rom_size = (raw[5] as u32) * (CHR_BANK_LEN as u32);

                let required_bytes =
                    INES_HEADER_LEN + trainer_size + (prg_rom_size + chr_rom_size) as usize;

                if required_bytes > total_len {
                    return Err(CartridgeError::TruncatedData);
                }

                let inferred_prg_ram_size = if has_trusted_ines_extension {
                    decode_ines_prg_ram_size(raw[8]) as u32
                } else {
                    0x2000
                };

                let has_prg_ram = !has_trusted_ines_extension || (raw[10] & 0x10) == 0;
                let timing_mode = if has_trusted_ines_extension {
                    TimingMode::decode_ines(raw[9], raw[10])
                } else {
                    TimingMode::NTSC
                };
                let tv_system = timing_mode.to_tv_system();
                let has_bus_conflicts = has_trusted_ines_extension && (raw[10] & 0x20 != 0);

                Ok(CartridgeHeader {
                    raw,
                    format,
                    mapper_id,
                    submapper: 0,
                    mirroring,
                    has_battery_backed_ram: has_sram,
                    has_trainer,
                    has_bus_conflicts,
                    console_type,
                    console_type_data,
                    timing_mode,
                    tv_system,
                    prg_rom_size,
                    prg_ram_size: if has_prg_ram && !has_sram {
                        inferred_prg_ram_size
                    } else {
                        0
                    },
                    prg_nvram_size: if has_prg_ram && has_sram {
                        inferred_prg_ram_size
                    } else {
                        0
                    },
                    chr_rom_size,
                    chr_ram_size: if chr_rom_size == 0 {
                        CHR_BANK_LEN as u32
                    } else {
                        0
                    },
                    chr_nvram_size: 0,
                    misc_rom_count: 0,
                    defaut_expansion_device: 0,
                    has_prg_ram_info: has_trusted_ines_extension,
                    uses_exponent_rom_size_encoding: false,
                })
            }
        };
    }
}

#[allow(dead_code)]
pub struct Cartridge {
    mapper: Box<dyn Mapper>,
    expansion_chips: Vec<Box<dyn ExpansionAudioChip>>,
    header: CartridgeHeader,
}

impl Cartridge {
    pub fn from_ines(rom: &[u8]) -> Result<Self, CartridgeError> {
        let header = CartridgeHeader::parse(rom)?;

        let flags6 = rom[6];
        let has_trainer = (flags6 & 0x04) != 0;
        let trainer_len = if has_trainer { TRAINER_LEN } else { 0 };

        let prg_len = rom[4] as usize * PRG_BANK_LEN;
        let chr_len = rom[5] as usize * CHR_BANK_LEN;
        let data_start = INES_HEADER_LEN + trainer_len;
        let data_end = data_start + prg_len + chr_len;
        if rom.len() < data_end {
            return Err(CartridgeError::TruncatedData);
        }

        let prg_rom = rom[data_start..data_start + prg_len].to_vec();
        let chr_rom = rom[data_start + prg_len..data_end].to_vec();
        let (mapper, expansion_chips) =
            from_mapper_id(header.mapper_id, header.mirroring, prg_rom, chr_rom)?;

        Ok(Self {
            mapper,
            expansion_chips,
            header,
        })
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mapper.mirroring()
    }

    pub fn tv_system(&self) -> TVSystem {
        self.header.tv_system
    }

    pub fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        self.mapper.cpu_read(addr)
    }

    pub fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        self.mapper.cpu_write(addr, data)
    }

    pub fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        self.mapper.ppu_read(addr)
    }

    pub fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        self.mapper.ppu_write(addr, data)
    }

    pub fn take_expansion_audio_chips(&mut self) -> Vec<Box<dyn ExpansionAudioChip>> {
        std::mem::take(&mut self.expansion_chips)
    }

    pub fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        self.mapper.check_a12(addr, ppu_cycle);
    }

    pub fn map_nametable_addr(&self, addr: u16) -> Option<usize> {
        self.mapper.map_nametable_addr(addr)
    }

    pub fn irq_line(&self) -> bool {
        self.mapper.irq_line()
    }

    pub fn tick_cpu_cycle(&mut self) {
        self.mapper.tick_cpu_cycle();
    }

    pub fn notify_scanline(&mut self, scanline: i16, rendering_on: bool) {
        self.mapper.notify_scanline(scanline, rendering_on);
    }

    pub fn set_ppu_sprite_phase(&mut self, sprite_phase: bool) {
        self.mapper.set_ppu_sprite_phase(sprite_phase);
    }

    pub fn ppu_read_nametable(&mut self, addr: u16) -> Option<u8> {
        self.mapper.ppu_read_nametable(addr)
    }

    pub fn ppu_write_nametable(&mut self, addr: u16, data: u8) -> bool {
        self.mapper.ppu_write_nametable(addr, data)
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u16(self.header.mapper_id);
        self.mapper.save_state(writer);
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        let actual = reader.read_u16()?;
        let expected = self.header.mapper_id;
        if actual != expected {
            return Err(SaveStateError::MapperMismatch { expected, actual });
        }
        self.mapper.load_state(reader)
    }
}

#[cfg(test)]
mod tests;
