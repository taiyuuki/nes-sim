mod anrom;
mod bnrom;
mod camerica;
mod cnrom;
mod colordreams;
mod gxrom;
mod mapper87;
mod mapper118;
mod mmc1;
mod mmc3;
mod nrom;
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
use self::gxrom::Gxrom;
use self::mapper87::Mapper87;
use self::mapper118::Mapper118;
use self::mmc1::Mmc1;
use self::mmc3::Mmc3;
use self::nrom::Nrom;
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
    fn save_state(&self, writer: &mut StateWriter);
    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError>;
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
        7 => Ok((Box::new(Anrom::new(prg_rom, chr_rom, mirroring)), vec![])),
        11 => Ok((
            Box::new(ColorDreams::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        22 => Ok((
            Box::new(Vrc2::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        21 | 23 | 25 => Ok((
            Box::new(Vrc4::new(prg_rom, chr_rom, mirroring, mapper_id)),
            vec![],
        )),
        24 | 26 => Ok(new_vrc6(prg_rom, chr_rom, mirroring, mapper_id)),
        34 => Ok((
            Box::new(Bnrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        66 => Ok((
            Box::new(Gxrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        71 => Ok((
            Box::new(Camerica::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        87 => Ok((
            Box::new(Mapper87::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        118 => Ok((
            Box::new(Mapper118::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        119 => Ok((
            Box::new(Tqrom::new(prg_rom, chr_rom, mirroring)),
            vec![],
        )),
        _ => Err(CartridgeError::UnsupportedMapper(mapper_id)),
    }
}
