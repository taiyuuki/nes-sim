mod anrom;
mod bandai;
mod bitcorp38;
mod bnrom;
mod camerica;
mod cnrom;
mod colordreams;
mod cprom;
mod crazy_climber;
mod fme7;
mod gxrom;
mod irem76;
mod irem_g101;
mod irem_h3001;
mod irem_tams1;
mod jaleco140;
mod jf13;
mod jf19;
mod mapper105;
mod mapper107;
mod mapper115;
mod mapper118;
mod mapper15;
mod mapper152;
mod mapper162;
mod mapper182;
mod mapper200;
mod mapper201;
mod mapper206;
mod mapper225;
mod mapper226;
mod mapper229;
mod mapper240;
mod mapper241;
mod mapper242;
mod mapper244;
mod mapper36;
mod mapper37;
mod mapper46;
mod mapper47;
mod mapper49;
mod mapper62;
mod mapper70;
mod mapper72;
mod mapper74;
mod mapper78;
mod mapper87;
mod mapper94;

mod mmc1;
mod mmc2;
mod mmc3;
mod mmc4;
mod mmc5;
mod namco163;
mod namco3433;
mod nina003;
mod nrom;
mod ntdec112;
mod rambo1;
mod ss8805;
mod sunsoft1;
mod sunsoft2;
mod sunsoft3;
mod sunsoft4;
mod taito0190;
mod taito_x1005;
mod taito_x1017;
mod tqrom;
mod uxrom;
mod vrc1;
mod vrc2;
mod vrc3;
mod vrc4;
mod vrc6;
mod vrc7;

use self::anrom::Anrom;
use self::bandai::Bandai;
use self::bitcorp38::BitCorp38;
use self::bnrom::Bnrom;
use self::camerica::Camerica;
use self::cnrom::Cnrom;
use self::colordreams::ColorDreams;
use self::cprom::CpROM;
use self::crazy_climber::CrazyClimber;
use self::fme7::{Fme7, new_fme7};
use self::gxrom::Gxrom;
use self::irem_g101::IremG101;
use self::irem_h3001::IremH3001;
use self::irem_tams1::IremTamS1;
use self::irem76::Irem76;
use self::jaleco140::Jaleco140;
use self::jf13::Jf13;
use self::jf19::Jf19;
use self::mapper15::Mapper15;
use self::mapper36::Mapper36;
use self::mapper37::Mapper37;
use self::mapper46::Mapper46;
use self::mapper47::Mapper47;
use self::mapper49::Mapper49;
use self::mapper62::Mapper62;
use self::mapper70::Mapper70;
use self::mapper72::Mapper72;
use self::mapper74::Mapper74;
use self::mapper78::Mapper78;
use self::mapper87::Mapper87;
use self::mapper94::Mapper94;
use self::mapper105::Mapper105;
use self::mapper107::Mapper107;
use self::mapper115::Mapper115;
use self::mapper118::Mapper118;
use self::mapper152::Mapper152;
use self::mapper162::Mapper162;
use self::mapper182::Mapper182;
use self::mapper200::Mapper200;
use self::mapper201::Mapper201;
use self::mapper206::Mapper206;
use self::mapper225::Mapper225;
use self::mapper226::Mapper226;
use self::mapper229::Mapper229;
use self::mapper240::Mapper240;
use self::mapper241::Mapper241;
use self::mapper242::Mapper242;
use self::mapper244::Mapper244;
use self::mmc1::Mmc1;
use self::mmc2::Mmc2;
use self::mmc3::Mmc3;
use self::mmc4::Mmc4;
use self::mmc5::{Mmc5, mmc5_wram_banks, new_mmc5};
use self::namco163::{Namco163, new_namco163};
use self::namco3433::Namco3433;
use self::nina003::Nina003;
use self::nrom::Nrom;
use self::ntdec112::Ntdec112;
use self::rambo1::Rambo1;
use self::ss8805::Ss8805;
use self::sunsoft1::{Sunsoft1, Sunsoft184, Sunsoft185};
use self::sunsoft2::Sunsoft2;
use self::sunsoft3::Sunsoft3;
use self::sunsoft4::Sunsoft4;
use self::taito_x1005::TaitoX1005;
use self::taito_x1017::TaitoX1017;
use self::taito0190::Taito0190;
use self::tqrom::Tqrom;
use self::uxrom::Uxrom;
use self::vrc1::Vrc1;
use self::vrc2::Vrc2;
use self::vrc3::Vrc3;
use self::vrc4::Vrc4;
use self::vrc6::{Vrc6, new_vrc6};
use self::vrc7::{Vrc7, new_vrc7};
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
    fn ppu_register_write(&mut self, _addr: u16, _data: u8) {}
    fn ppu_read_nametable(&mut self, _addr: u16) -> Option<u8> {
        None
    }
    fn ppu_write_nametable(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }
    fn save_state(&self, _writer: &mut StateWriter) {}
    fn load_state(&mut self, _reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        Ok(())
    }
}

pub struct NoMapper {}

impl NoMapper {
    pub fn new() -> Self {
        Self {}
    }
}

impl Mapper for NoMapper {
    fn cpu_read(&mut self, _addr: u16) -> Option<u8> {
        None
    }

    fn cpu_write(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }

    fn ppu_read(&mut self, _addr: u16) -> Option<u8> {
        None
    }

    fn ppu_write(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }

    fn mirroring(&self) -> Mirroring {
        Mirroring::Vertical
    }
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
            Self::NoMapper(m) => m.$method($($arg),*),
            Self::Nrom(m) => m.$method($($arg),*),
            Self::Mmc1(m) => m.$method($($arg),*),
            Self::Mmc2(m) => m.$method($($arg),*),
            Self::Mmc4(m) => m.$method($($arg),*),
            Self::Uxrom(m) => m.$method($($arg),*),
            Self::Cnrom(m) => m.$method($($arg),*),
            Self::Mmc3(m) => m.$method($($arg),*),
            Self::Mapper74(m) => m.$method($($arg),*),
            Self::Mmc5(m) => m.$method($($arg),*),
            Self::Anrom(m) => m.$method($($arg),*),
            Self::ColorDreams(m) => m.$method($($arg),*),
            Self::CpROM(m) => m.$method($($arg),*),
            Self::Namco163(m) => m.$method($($arg),*),
            Self::Vrc1(m) => m.$method($($arg),*),
            Self::Vrc2(m) => m.$method($($arg),*),
            Self::Vrc3(m) => m.$method($($arg),*),
            Self::Vrc4(m) => m.$method($($arg),*),
            Self::Vrc6(m) => m.$method($($arg),*),
            Self::Vrc7(m) => m.$method($($arg),*),
            Self::Rambo1(m) => m.$method($($arg),*),
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
            Self::Sunsoft1(m) => m.$method($($arg),*),
            Self::Sunsoft184(m) => m.$method($($arg),*),
            Self::Sunsoft185(m) => m.$method($($arg),*),
            Self::Sunsoft2(m) => m.$method($($arg),*),
            Self::Sunsoft3(m) => m.$method($($arg),*),
            Self::Sunsoft4(m) => m.$method($($arg),*),
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
            Self::Ss8805(m) => m.$method($($arg),*),
            Self::Bandai(m) => m.$method($($arg),*),
            Self::Jaleco140(m) => m.$method($($arg),*),
            Self::CrazyClimber(m) => m.$method($($arg),*),
            Self::Ntdec112(m) => m.$method($($arg),*),
            Self::Mapper107(m) => m.$method($($arg),*),
            Self::Mapper15(m) => m.$method($($arg),*),
            Self::Mapper182(m) => m.$method($($arg),*),
            Self::BitCorp38(m) => m.$method($($arg),*),
            Self::Mapper36(m) => m.$method($($arg),*),
            Self::Mapper46(m) => m.$method($($arg),*),
            Self::Mapper62(m) => m.$method($($arg),*),
            Self::Mapper72(m) => m.$method($($arg),*),
            Self::Mapper94(m) => m.$method($($arg),*),
            Self::Mapper115(m) => m.$method($($arg),*),
            Self::Mapper152(m) => m.$method($($arg),*),
            Self::Mapper162(m) => m.$method($($arg),*),
            Self::Mapper37(m) => m.$method($($arg),*),
            Self::Mapper47(m) => m.$method($($arg),*),
            Self::Mapper49(m) => m.$method($($arg),*),
            Self::Mapper105(m) => m.$method($($arg),*),
            Self::Mapper206(m) => m.$method($($arg),*),
            Self::Mapper200(m) => m.$method($($arg),*),
            Self::Mapper201(m) => m.$method($($arg),*),
            Self::Mapper225(m) => m.$method($($arg),*),
            Self::Mapper226(m) => m.$method($($arg),*),
            Self::Mapper229(m) => m.$method($($arg),*),
            Self::Mapper240(m) => m.$method($($arg),*),
            Self::Mapper241(m) => m.$method($($arg),*),
            Self::Mapper242(m) => m.$method($($arg),*),
            Self::Mapper244(m) => m.$method($($arg),*),
        }
    };
}

#[allow(private_interfaces)]
pub(super) enum MapperEnum {
    NoMapper(NoMapper),
    Nrom(Nrom),
    Mmc1(Mmc1),
    Mmc2(Mmc2),
    Mmc4(Mmc4),
    Uxrom(Uxrom),
    Cnrom(Cnrom),
    Mmc3(Mmc3),
    Mapper74(Mapper74),
    Mmc5(Mmc5),
    Anrom(Anrom),
    ColorDreams(ColorDreams),
    CpROM(CpROM),
    Namco163(Namco163),
    Vrc1(Vrc1),
    Vrc2(Vrc2),
    Vrc3(Vrc3),
    Vrc4(Vrc4),
    Vrc6(Vrc6),
    Vrc7(Vrc7),
    Rambo1(Rambo1),
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
    Sunsoft1(Sunsoft1),
    Sunsoft184(Sunsoft184),
    Sunsoft185(Sunsoft185),
    Sunsoft2(Sunsoft2),
    Sunsoft3(Sunsoft3),
    Sunsoft4(Sunsoft4),
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
    Ss8805(Ss8805),
    Bandai(Bandai),
    Jaleco140(Jaleco140),
    CrazyClimber(CrazyClimber),
    Ntdec112(Ntdec112),
    Mapper107(Mapper107),
    Mapper15(Mapper15),
    Mapper182(Mapper182),
    BitCorp38(BitCorp38),
    Mapper36(Mapper36),
    Mapper46(Mapper46),
    Mapper62(Mapper62),
    Mapper72(Mapper72),
    Mapper94(Mapper94),
    Mapper115(Mapper115),
    Mapper152(Mapper152),
    Mapper162(Mapper162),
    Mapper37(Mapper37),
    Mapper47(Mapper47),
    Mapper49(Mapper49),
    Mapper105(Mapper105),
    Mapper206(Mapper206),
    Mapper200(Mapper200),
    Mapper201(Mapper201),
    Mapper225(Mapper225),
    Mapper226(Mapper226),
    Mapper229(Mapper229),
    Mapper240(Mapper240),
    Mapper241(Mapper241),
    Mapper242(Mapper242),
    Mapper244(Mapper244),
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

    pub(super) fn ppu_register_write(&mut self, addr: u16, data: u8) {
        dispatch_mapper!(self, ppu_register_write(addr, data))
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

// PRG+CHR拼接数据的CRC32（与fceux卡带识别使用的校验范围一致）
fn crc32_concat(prg: &[u8], chr: &[u8]) -> u32 {
    fn crc32_update(crc: u32, bytes: &[u8]) -> u32 {
        let mut crc = !crc;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB88320 & mask);
            }
        }
        !crc
    }
    crc32_update(crc32_update(0, prg), chr)
}

pub(super) fn from_mapper_id(
    mapper_id: u16,
    mirroring: Mirroring,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram_len: usize,
) -> Result<(MapperEnum, Vec<Box<dyn ExpansionAudioChip>>), CartridgeError> {
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
            let crc32 = crc32_concat(&prg_rom, &chr_rom);
            let (mapper, chips) = new_mmc5(
                prg_rom,
                chr_rom,
                mirroring,
                mmc5_wram_banks(crc32),
                chr_ram_len,
            );
            Ok((MapperEnum::Mmc5(mapper), chips))
        }
        7 => Ok((
            MapperEnum::Anrom(Anrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        9 => Ok((
            MapperEnum::Mmc2(Mmc2::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        10 => Ok((
            MapperEnum::Mmc4(Mmc4::new(prg_rom, chr_rom, mirroring)),
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
        15 => Ok((
            MapperEnum::Mapper15(Mapper15::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        16 | 159 => Ok((
            MapperEnum::Bandai(Bandai::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        18 => Ok((
            MapperEnum::Ss8805(Ss8805::new(prg_rom, chr_rom, mirroring)),
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
        32 => Ok((
            MapperEnum::IremG101(IremG101::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        33 => Ok((
            MapperEnum::Taito0190(Taito0190::new(prg_rom, chr_rom, mirroring, false)),
            vec![],
        )),
        34 => Ok((
            MapperEnum::Bnrom(Bnrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        36 => Ok((
            MapperEnum::Mapper36(Mapper36::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        37 => Ok((
            MapperEnum::Mapper37(Mapper37::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        38 => Ok((
            MapperEnum::BitCorp38(BitCorp38::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        46 => Ok((
            MapperEnum::Mapper46(Mapper46::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        47 => Ok((
            MapperEnum::Mapper47(Mapper47::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        49 => Ok((
            MapperEnum::Mapper49(Mapper49::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        48 => Ok((
            MapperEnum::Taito0190(Taito0190::new(prg_rom, chr_rom, mirroring, true)),
            vec![],
        )),
        62 => Ok((
            MapperEnum::Mapper62(Mapper62::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        64 => Ok((
            MapperEnum::Rambo1(Rambo1::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        65 => Ok((
            MapperEnum::IremH3001(IremH3001::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        66 => Ok((
            MapperEnum::Gxrom(Gxrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        67 => Ok((
            MapperEnum::Sunsoft3(Sunsoft3::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        68 => Ok((
            MapperEnum::Sunsoft4(Sunsoft4::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        69 => {
            let (mapper, chips) = new_fme7(prg_rom, chr_rom, mirroring);
            Ok((MapperEnum::Fme7(mapper), chips))
        }
        70 => Ok((
            MapperEnum::Mapper70(Mapper70::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        71 => Ok((
            MapperEnum::Camerica(Camerica::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        72 => Ok((
            MapperEnum::Mapper72(Mapper72::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        73 => Ok((
            MapperEnum::Vrc3(Vrc3::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        74 => Ok((
            MapperEnum::Mapper74(Mapper74::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        75 => Ok((
            MapperEnum::Vrc1(Vrc1::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        76 => Ok((
            MapperEnum::Irem76(Irem76::new(prg_rom, chr_rom, mirroring)),
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
        80 => Ok((
            MapperEnum::TaitoX1005(TaitoX1005::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        82 => Ok((
            MapperEnum::TaitoX1017(TaitoX1017::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        85 => {
            let (mapper, chips) = new_vrc7(prg_rom, chr_rom, mirroring);
            Ok((MapperEnum::Vrc7(mapper), chips))
        }
        86 => Ok((
            MapperEnum::Jf13(Jf13::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        87 => Ok((
            MapperEnum::Mapper87(Mapper87::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        88 => Ok((
            MapperEnum::Namco3433(Namco3433::new(prg_rom, chr_rom, mirroring, false)),
            vec![],
        )),
        89 => Ok((
            MapperEnum::Sunsoft2(Sunsoft2::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        92 => Ok((
            MapperEnum::Jf19(Jf19::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        93 => Ok((
            MapperEnum::Sunsoft1(Sunsoft1::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        94 => Ok((
            MapperEnum::Mapper94(Mapper94::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        97 => Ok((
            MapperEnum::IremTamS1(IremTamS1::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        105 => Ok((
            MapperEnum::Mapper105(Mapper105::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        107 => Ok((
            MapperEnum::Mapper107(Mapper107::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        112 => Ok((
            MapperEnum::Ntdec112(Ntdec112::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        113 => Ok((
            MapperEnum::Nina003(Nina003::new(prg_rom, chr_rom, mirroring, true)),
            vec![],
        )),
        115 => Ok((
            MapperEnum::Mapper115(Mapper115::new(prg_rom, chr_rom, mirroring)),
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
        140 => Ok((
            MapperEnum::Jaleco140(Jaleco140::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        152 => Ok((
            MapperEnum::Mapper152(Mapper152::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        154 => Ok((
            MapperEnum::Namco3433(Namco3433::new(prg_rom, chr_rom, mirroring, true)),
            vec![],
        )),
        162 => Ok((
            MapperEnum::Mapper162(Mapper162::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        180 => Ok((
            MapperEnum::CrazyClimber(CrazyClimber::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        182 => Ok((
            MapperEnum::Mapper182(Mapper182::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        184 => Ok((
            MapperEnum::Sunsoft184(Sunsoft184::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        185 => Ok((
            MapperEnum::Sunsoft185(Sunsoft185::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        200 => Ok((
            MapperEnum::Mapper200(Mapper200::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        201 => Ok((
            MapperEnum::Mapper201(Mapper201::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        206 => Ok((
            MapperEnum::Mapper206(Mapper206::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        225 => Ok((
            MapperEnum::Mapper225(Mapper225::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        226 => Ok((
            MapperEnum::Mapper226(Mapper226::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        229 => Ok((
            MapperEnum::Mapper229(Mapper229::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        240 => Ok((
            MapperEnum::Mapper240(Mapper240::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        241 => Ok((
            MapperEnum::Mapper241(Mapper241::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        242 => Ok((
            MapperEnum::Mapper242(Mapper242::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        244 => Ok((
            MapperEnum::Mapper244(Mapper244::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),

        _ => Err(CartridgeError::UnsupportedMapper(mapper_id)),
    }
}
