mod anrom;
mod bnrom;
mod camerica;
mod cnrom;
mod colordreams;
mod cprom;
mod fme7;
mod gxrom;
mod irem76;
mod irem_g101;
mod irem_h3001;
mod irem_tams1;
mod jf13;
mod jf19;
mod mapper115;
mod mapper118;
mod mapper152;
mod mapper162;
mod mapper36;
mod mapper46;
mod mapper62;
mod mapper70;
mod mapper72;
mod mapper78;
mod mapper87;
mod mapper94;

mod mmc1;
mod mmc3;
mod mmc5;
mod namco163;
mod namco3433;
mod nina003;
mod nrom;
mod sunsoft3;
mod taito0190;
mod taito_x1005;
mod taito_x1017;
mod tqrom;
mod uxrom;
mod vrc2;
mod vrc4;
mod vrc6;

use self::anrom::Anrom;
use self::bnrom::Bnrom;
use self::camerica::Camerica;
use self::cnrom::Cnrom;
use self::colordreams::ColorDreams;
use self::cprom::CpROM;
use self::fme7::{Fme7, new_fme7};
use self::gxrom::Gxrom;
use self::irem_g101::IremG101;
use self::irem_h3001::IremH3001;
use self::irem_tams1::IremTamS1;
use self::irem76::Irem76;
use self::jf13::Jf13;
use self::jf19::Jf19;
use self::mapper36::Mapper36;
use self::mapper46::Mapper46;
use self::mapper62::Mapper62;
use self::mapper70::Mapper70;
use self::mapper72::Mapper72;
use self::mapper78::Mapper78;
use self::mapper87::Mapper87;
use self::mapper94::Mapper94;
use self::mapper115::Mapper115;
use self::mapper118::Mapper118;
use self::mapper152::Mapper152;
use self::mapper162::Mapper162;
use self::mmc1::Mmc1;
use self::mmc3::Mmc3;
use self::mmc5::{Mmc5, new_mmc5};
use self::namco163::{Namco163, new_namco163};
use self::namco3433::Namco3433;
use self::nina003::Nina003;
use self::nrom::Nrom;
use self::sunsoft3::Sunsoft3;
use self::taito_x1005::TaitoX1005;
use self::taito_x1017::TaitoX1017;
use self::taito0190::Taito0190;
use self::tqrom::Tqrom;
use self::uxrom::Uxrom;
use self::vrc2::Vrc2;
use self::vrc4::Vrc4;
use self::vrc6::{Vrc6, new_vrc6};
use super::{CartridgeError, Mirroring};
use crate::apu::ExpansionAudioChip;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

pub(super) trait Mapper {
    fn cpu_read(&mut self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, data: u8) -> bool;
    fn ppu_read(&mut self, addr: u16) -> Option<u8>;
    fn ppu_write(&mut self, addr: u16, data: u8) -> bool;
    fn mirroring(&self) -> Mirroring;
    fn map_nametable_addr(&self, _addr: u16) -> Option<usize> {
        None
    }
    fn check_a12(&mut self, _addr: u16, _ppu_cycle: u64) {}
    fn irq_line(&self) -> bool {
        false
    }
    fn tick_cpu_cycle(&mut self) {}
    fn notify_scanline(&mut self, _scanline: i16, _rendering_on: bool) {}
    fn set_ppu_sprite_phase(&mut self, _sprite_phase: bool) {}
    fn ppu_read_nametable(&mut self, _addr: u16) -> Option<u8> {
        None
    }
    fn ppu_write_nametable(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }
    fn save_state(&self, writer: &mut StateWriter);
    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError>;
}

pub fn encode_mirroring(mirroring: Mirroring) -> u8 {
    match mirroring {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::FourScreen => 2,
        Mirroring::SPAGE0 => 3,
        Mirroring::SPAGE1 => 4,
    }
}

pub fn decode_mirroring(encoded: u8) -> Result<Mirroring, SaveStateError> {
    match encoded {
        0 => Ok(Mirroring::Horizontal),
        1 => Ok(Mirroring::Vertical),
        2 => Ok(Mirroring::FourScreen),
        3 => Ok(Mirroring::SPAGE0),
        4 => Ok(Mirroring::SPAGE1),
        _ => Err(SaveStateError::InvalidData(
            "invalid MMC118 mirroring value",
        )),
    }
}

macro_rules! dispatch_mapper {
    ($self:expr, $method:ident($($arg:expr),*)) => {
        match $self {
            Self::Nrom(m) => m.$method($($arg),*),
            Self::Mmc1(m) => m.$method($($arg),*),
            Self::Uxrom(m) => m.$method($($arg),*),
            Self::Cnrom(m) => m.$method($($arg),*),
            Self::Mmc3(m) => m.$method($($arg),*),
            Self::Mmc5(m) => m.$method($($arg),*),
            Self::Anrom(m) => m.$method($($arg),*),
            Self::ColorDreams(m) => m.$method($($arg),*),
            Self::CpROM(m) => m.$method($($arg),*),
            Self::Namco163(m) => m.$method($($arg),*),
            Self::Vrc2(m) => m.$method($($arg),*),
            Self::Vrc4(m) => m.$method($($arg),*),
            Self::Vrc6(m) => m.$method($($arg),*),
            Self::Bnrom(m) => m.$method($($arg),*),
            Self::Gxrom(m) => m.$method($($arg),*),
            Self::Fme7(m) => m.$method($($arg),*),
            Self::Camerica(m) => m.$method($($arg),*),
            Self::Mapper78(m) => m.$method($($arg),*),
            Self::Nina003(m) => m.$method($($arg),*),
            Self::Mapper87(m) => m.$method($($arg),*),
            Self::Mapper118(m) => m.$method($($arg),*),
            Self::Tqrom(m) => m.$method($($arg),*),
            Self::Taito0190(m) => m.$method($($arg),*),
            Self::Sunsoft3(m) => m.$method($($arg),*),
            Self::TaitoX1005(m) => m.$method($($arg),*),
            Self::Namco3433(m) => m.$method($($arg),*),
            Self::IremG101(m) => m.$method($($arg),*),
            Self::IremH3001(m) => m.$method($($arg),*),
            Self::Irem76(m) => m.$method($($arg),*),
            Self::Jf13(m) => m.$method($($arg),*),
            Self::Mapper70(m) => m.$method($($arg),*),
            Self::TaitoX1017(m) => m.$method($($arg),*),
            Self::Jf19(m) => m.$method($($arg),*),
            Self::IremTamS1(m) => m.$method($($arg),*),
            Self::Mapper36(m) => m.$method($($arg),*),
            Self::Mapper46(m) => m.$method($($arg),*),
            Self::Mapper62(m) => m.$method($($arg),*),
            Self::Mapper72(m) => m.$method($($arg),*),
            Self::Mapper94(m) => m.$method($($arg),*),
            Self::Mapper115(m) => m.$method($($arg),*),
            Self::Mapper152(m) => m.$method($($arg),*),
            Self::Mapper162(m) => m.$method($($arg),*),
        }
    };
}

#[allow(private_interfaces)]
pub(super) enum MapperEnum {
    Nrom(Nrom),
    Mmc1(Mmc1),
    Uxrom(Uxrom),
    Cnrom(Cnrom),
    Mmc3(Mmc3),
    Mmc5(Mmc5),
    Anrom(Anrom),
    ColorDreams(ColorDreams),
    CpROM(CpROM),
    Namco163(Namco163),
    Vrc2(Vrc2),
    Vrc4(Vrc4),
    Vrc6(Vrc6),
    Bnrom(Bnrom),
    Gxrom(Gxrom),
    Fme7(Fme7),
    Camerica(Camerica),
    Mapper78(Mapper78),
    Nina003(Nina003),
    Mapper87(Mapper87),
    Mapper118(Mapper118),
    Tqrom(Tqrom),
    Taito0190(Taito0190),
    Sunsoft3(Sunsoft3),
    TaitoX1005(TaitoX1005),
    Namco3433(Namco3433),
    IremG101(IremG101),
    IremH3001(IremH3001),
    Irem76(Irem76),
    Jf13(Jf13),
    Mapper70(Mapper70),
    TaitoX1017(TaitoX1017),
    Jf19(Jf19),
    IremTamS1(IremTamS1),
    Mapper36(Mapper36),
    Mapper46(Mapper46),
    Mapper62(Mapper62),
    Mapper72(Mapper72),
    Mapper94(Mapper94),
    Mapper115(Mapper115),
    Mapper152(Mapper152),
    Mapper162(Mapper162),
}

impl MapperEnum {
    #[inline]
    pub(super) fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        dispatch_mapper!(self, cpu_read(addr))
    }

    #[inline]
    pub(super) fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        dispatch_mapper!(self, cpu_write(addr, data))
    }

    #[inline]
    pub(super) fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        dispatch_mapper!(self, ppu_read(addr))
    }

    #[inline]
    pub(super) fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        dispatch_mapper!(self, ppu_write(addr, data))
    }

    pub(super) fn mirroring(&self) -> Mirroring {
        dispatch_mapper!(self, mirroring())
    }

    pub(super) fn map_nametable_addr(&self, addr: u16) -> Option<usize> {
        dispatch_mapper!(self, map_nametable_addr(addr))
    }

    pub(super) fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        dispatch_mapper!(self, check_a12(addr, ppu_cycle))
    }

    pub(super) fn irq_line(&self) -> bool {
        dispatch_mapper!(self, irq_line())
    }

    pub(super) fn tick_cpu_cycle(&mut self) {
        dispatch_mapper!(self, tick_cpu_cycle())
    }

    pub(super) fn notify_scanline(&mut self, scanline: i16, rendering_on: bool) {
        dispatch_mapper!(self, notify_scanline(scanline, rendering_on))
    }

    pub(super) fn set_ppu_sprite_phase(&mut self, sprite_phase: bool) {
        dispatch_mapper!(self, set_ppu_sprite_phase(sprite_phase))
    }

    pub(super) fn ppu_read_nametable(&mut self, addr: u16) -> Option<u8> {
        dispatch_mapper!(self, ppu_read_nametable(addr))
    }

    pub(super) fn ppu_write_nametable(&mut self, addr: u16, data: u8) -> bool {
        dispatch_mapper!(self, ppu_write_nametable(addr, data))
    }

    pub(super) fn save_state(&self, writer: &mut StateWriter) {
        dispatch_mapper!(self, save_state(writer))
    }

    pub(super) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        dispatch_mapper!(self, load_state(reader))
    }
}

pub(super) fn from_mapper_id(
    mapper_id: u16,
    mirroring: Mirroring,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
) -> Result<(MapperEnum, Vec<Box<dyn ExpansionAudioChip>>), CartridgeError> {
    println!("{mapper_id}");
    match mapper_id {
        0 => Ok((
            MapperEnum::Nrom(Nrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        1 => Ok((
            MapperEnum::Mmc1(Mmc1::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        2 => Ok((
            MapperEnum::Uxrom(Uxrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        3 => Ok((
            MapperEnum::Cnrom(Cnrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        4 => Ok((
            MapperEnum::Mmc3(Mmc3::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        5 => {
            let (mapper, chips) = new_mmc5(prg_rom, chr_rom, mirroring);
            Ok((MapperEnum::Mmc5(mapper), chips))
        }
        7 => Ok((
            MapperEnum::Anrom(Anrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        11 => Ok((
            MapperEnum::ColorDreams(ColorDreams::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        13 => Ok((
            MapperEnum::CpROM(CpROM::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        19 => {
            let (mapper, chips) = new_namco163(prg_rom, chr_rom, mirroring);
            Ok((MapperEnum::Namco163(mapper), chips))
        }
        22 => Ok((
            MapperEnum::Vrc2(Vrc2::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        21 | 23 | 25 => Ok((
            MapperEnum::Vrc4(Vrc4::new(prg_rom, chr_rom, mirroring, mapper_id)),
            vec![],
        )),
        24 | 26 => {
            let (mapper, chips) = new_vrc6(prg_rom, chr_rom, mirroring, mapper_id);
            Ok((MapperEnum::Vrc6(mapper), chips))
        }
        34 => Ok((
            MapperEnum::Bnrom(Bnrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        66 => Ok((
            MapperEnum::Gxrom(Gxrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        69 => {
            let (mapper, chips) = new_fme7(prg_rom, chr_rom, mirroring);
            Ok((MapperEnum::Fme7(mapper), chips))
        }
        71 => Ok((
            MapperEnum::Camerica(Camerica::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        78 => Ok((
            MapperEnum::Mapper78(Mapper78::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        79 => Ok((
            MapperEnum::Nina003(Nina003::new(prg_rom, chr_rom, mirroring, false)),
            vec![],
        )),
        87 => Ok((
            MapperEnum::Mapper87(Mapper87::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        113 => Ok((
            MapperEnum::Nina003(Nina003::new(prg_rom, chr_rom, mirroring, true)),
            vec![],
        )),
        118 => Ok((
            MapperEnum::Mapper118(Mapper118::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        119 => Ok((
            MapperEnum::Tqrom(Tqrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        33 => Ok((
            MapperEnum::Taito0190(Taito0190::new(prg_rom, chr_rom, mirroring, false)),
            vec![],
        )),
        48 => Ok((
            MapperEnum::Taito0190(Taito0190::new(prg_rom, chr_rom, mirroring, true)),
            vec![],
        )),
        67 => Ok((
            MapperEnum::Sunsoft3(Sunsoft3::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        80 => Ok((
            MapperEnum::TaitoX1005(TaitoX1005::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        88 => Ok((
            MapperEnum::Namco3433(Namco3433::new(prg_rom, chr_rom, mirroring, false)),
            vec![],
        )),
        154 => Ok((
            MapperEnum::Namco3433(Namco3433::new(prg_rom, chr_rom, mirroring, true)),
            vec![],
        )),
        32 => Ok((
            MapperEnum::IremG101(IremG101::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        65 => Ok((
            MapperEnum::IremH3001(IremH3001::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        76 => Ok((
            MapperEnum::Irem76(Irem76::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        86 => Ok((
            MapperEnum::Jf13(Jf13::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        70 => Ok((
            MapperEnum::Mapper70(Mapper70::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        82 => Ok((
            MapperEnum::TaitoX1017(TaitoX1017::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        92 => Ok((
            MapperEnum::Jf19(Jf19::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        97 => Ok((
            MapperEnum::IremTamS1(IremTamS1::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        36 => Ok((
            MapperEnum::Mapper36(Mapper36::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        46 => Ok((
            MapperEnum::Mapper46(Mapper46::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        62 => Ok((
            MapperEnum::Mapper62(Mapper62::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        72 => Ok((
            MapperEnum::Mapper72(Mapper72::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        94 => Ok((
            MapperEnum::Mapper94(Mapper94::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        115 => Ok((
            MapperEnum::Mapper115(Mapper115::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        152 => Ok((
            MapperEnum::Mapper152(Mapper152::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        162 => Ok((
            MapperEnum::Mapper162(Mapper162::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        _ => Err(CartridgeError::UnsupportedMapper(mapper_id)),
    }
}
