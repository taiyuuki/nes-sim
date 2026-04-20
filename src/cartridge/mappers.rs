mod anrom;
mod cnrom;
mod mapper118;
mod mmc1;
mod mmc3;
mod nrom;
mod uxrom;

use self::anrom::Anrom;
use self::cnrom::Cnrom;
use self::mapper118::Mapper118;
use self::mmc1::Mmc1;
use self::mmc3::Mmc3;
use self::nrom::Nrom;
use self::uxrom::Uxrom;
use super::{CartridgeError, Mirroring};
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
    fn save_state(&self, writer: &mut StateWriter);
    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError>;
}

pub(super) fn from_mapper_id(
    mapper_id: u16,
    mirroring: Mirroring,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
) -> Result<Box<dyn Mapper>, CartridgeError> {
    match mapper_id {
        0 => Ok(Box::new(Nrom::new(prg_rom, chr_rom, mirroring))),
        1 => Ok(Box::new(Mmc1::new(prg_rom, chr_rom, mirroring))),
        2 => Ok(Box::new(Uxrom::new(prg_rom, chr_rom, mirroring))),
        3 => Ok(Box::new(Cnrom::new(prg_rom, chr_rom, mirroring))),
        4 => Ok(Box::new(Mmc3::new(prg_rom, chr_rom, mirroring))),
        7 => Ok(Box::new(Anrom::new(prg_rom, chr_rom, mirroring))),
        118 => Ok(Box::new(Mapper118::new(prg_rom, chr_rom, mirroring))),
        _ => Err(CartridgeError::UnsupportedMapper(mapper_id)),
    }
}
