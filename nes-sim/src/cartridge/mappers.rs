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
use self::fme7::new_fme7;
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
use self::mmc5::new_mmc5;
use self::namco163::new_namco163;
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
use self::vrc6::new_vrc6;
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

pub(super) fn from_mapper_id(
    mapper_id: u16,
    mirroring: Mirroring,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
) -> Result<(Box<dyn Mapper>, Vec<Box<dyn ExpansionAudioChip>>), CartridgeError> {
    match mapper_id {
        0 => Ok((Box::new(Nrom::new(prg_rom, chr_rom, mirroring)), vec![])),
        1 => Ok((Box::new(Mmc1::new(prg_rom, chr_rom, mirroring)), vec![])),
        2 => Ok((Box::new(Uxrom::new(prg_rom, chr_rom, mirroring)), vec![])),
        3 => Ok((Box::new(Cnrom::new(prg_rom, chr_rom, mirroring)), vec![])),
        4 => Ok((Box::new(Mmc3::new(prg_rom, chr_rom, mirroring)), vec![])),
        5 => Ok(new_mmc5(prg_rom, chr_rom, mirroring)),
        7 => Ok((Box::new(Anrom::new(prg_rom, chr_rom, mirroring)), vec![])),
        11 => Ok((
            Box::new(ColorDreams::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        13 => Ok((Box::new(CpROM::new(prg_rom, chr_rom, mirroring)), vec![])),
        19 => Ok(new_namco163(prg_rom, chr_rom, mirroring)),
        22 => Ok((Box::new(Vrc2::new(prg_rom, chr_rom, mirroring)), vec![])),
        21 | 23 | 25 => Ok((
            Box::new(Vrc4::new(prg_rom, chr_rom, mirroring, mapper_id)),
            vec![],
        )),
        24 | 26 => Ok(new_vrc6(prg_rom, chr_rom, mirroring, mapper_id)),
        34 => Ok((Box::new(Bnrom::new(prg_rom, chr_rom, mirroring)), vec![])),
        66 => Ok((Box::new(Gxrom::new(prg_rom, chr_rom, mirroring)), vec![])),
        69 => Ok(new_fme7(prg_rom, chr_rom, mirroring)),
        71 => Ok((Box::new(Camerica::new(prg_rom, chr_rom, mirroring)), vec![])),
        78 => Ok((Box::new(Mapper78::new(prg_rom, chr_rom, mirroring)), vec![])),
        79 => Ok((
            Box::new(Nina003::new(prg_rom, chr_rom, mirroring, false)),
            vec![],
        )),
        87 => Ok((Box::new(Mapper87::new(prg_rom, chr_rom, mirroring)), vec![])),
        113 => Ok((
            Box::new(Nina003::new(prg_rom, chr_rom, mirroring, true)),
            vec![],
        )),
        118 => Ok((
            Box::new(Mapper118::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        119 => Ok((Box::new(Tqrom::new(prg_rom, chr_rom, mirroring)), vec![])),
        33 => Ok((
            Box::new(Taito0190::new(prg_rom, chr_rom, mirroring, false)),
            vec![],
        )),
        48 => Ok((
            Box::new(Taito0190::new(prg_rom, chr_rom, mirroring, true)),
            vec![],
        )),
        67 => Ok((Box::new(Sunsoft3::new(prg_rom, chr_rom, mirroring)), vec![])),
        80 => Ok((
            Box::new(TaitoX1005::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        88 => Ok((
            Box::new(Namco3433::new(prg_rom, chr_rom, mirroring, false)),
            vec![],
        )),
        154 => Ok((
            Box::new(Namco3433::new(prg_rom, chr_rom, mirroring, true)),
            vec![],
        )),
        32 => Ok((Box::new(IremG101::new(prg_rom, chr_rom, mirroring)), vec![])),
        65 => Ok((
            Box::new(IremH3001::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        76 => Ok((Box::new(Irem76::new(prg_rom, chr_rom, mirroring)), vec![])),
        86 => Ok((Box::new(Jf13::new(prg_rom, chr_rom, mirroring)), vec![])),
        70 => Ok((Box::new(Mapper70::new(prg_rom, chr_rom, mirroring)), vec![])),
        82 => Ok((
            Box::new(TaitoX1017::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        92 => Ok((Box::new(Jf19::new(prg_rom, chr_rom, mirroring)), vec![])),
        97 => Ok((
            Box::new(IremTamS1::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        36 => Ok((Box::new(Mapper36::new(prg_rom, chr_rom, mirroring)), vec![])),
        46 => Ok((Box::new(Mapper46::new(prg_rom, chr_rom, mirroring)), vec![])),
        62 => Ok((Box::new(Mapper62::new(prg_rom, chr_rom, mirroring)), vec![])),
        72 => Ok((Box::new(Mapper72::new(prg_rom, chr_rom, mirroring)), vec![])),
        94 => Ok((Box::new(Mapper94::new(prg_rom, chr_rom, mirroring)), vec![])),
        115 => Ok((
            Box::new(Mapper115::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        152 => Ok((
            Box::new(Mapper152::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        162 => Ok((
            Box::new(Mapper162::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        _ => Err(CartridgeError::UnsupportedMapper(mapper_id)),
    }
}
