use super::Mapper;
use super::mmc3::Mmc3Core;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const CHR_RAM_LEN: usize = 0x2000;

pub(super) struct Tqrom {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: Mirroring,
    core: Mmc3Core,
}

impl Tqrom {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr_rom,
            chr_ram: vec![0; CHR_RAM_LEN],
            mirroring,
            core: Mmc3Core::new(),
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K;
        let bank = self.core.prg_bank_number(self.prg_bank_count(), slot);
        bank * PRG_BANK_8K + ((addr as usize) & 0x1FFF)
    }
}

impl Mapper for Tqrom {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                if self.core.prg_ram_enabled() && !self.core.prg_ram_write_protect() {
                    self.prg_ram[(addr - 0x6000) as usize] = data;
                }
                true
            }
            0x8000..=0xFFFF => self
                .core
                .write_register(addr, data, Some(&mut self.mirroring)),
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let slot = addr as usize / CHR_BANK_1K;
        let bank = self.core.effective_chr_bank_value(slot) as usize;
        if bank & 0x40 != 0 {
            let offset = (bank & 0x07) * CHR_BANK_1K + (addr as usize & 0x03FF);
            Some(self.chr_ram[offset % CHR_RAM_LEN])
        } else {
            let bank_count = (self.chr_rom.len() / CHR_BANK_1K).max(1);
            let offset = (bank % bank_count) * CHR_BANK_1K + (addr as usize & 0x03FF);
            Some(self.chr_rom[offset % self.chr_rom.len()])
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = addr as usize / CHR_BANK_1K;
        let bank = self.core.effective_chr_bank_value(slot) as usize;
        if bank & 0x40 != 0 {
            let offset = (bank & 0x07) * CHR_BANK_1K + (addr as usize & 0x03FF);
            self.chr_ram[offset % CHR_RAM_LEN] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        self.core.check_a12(addr, ppu_cycle);
    }

    fn irq_line(&self) -> bool {
        self.core.irq_line()
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        writer.write_bytes(&self.chr_ram);
        writer.write_u8(encode_mirroring(self.mirroring));
        self.core.save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        reader.read_bytes_into(&mut self.chr_ram)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.core.load_state(reader)?;
        Ok(())
    }
}

fn encode_mirroring(mirroring: Mirroring) -> u8 {
    match mirroring {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::FourScreen => 2,
        Mirroring::SPAGE0 => 3,
        Mirroring::SPAGE1 => 4,
    }
}

fn decode_mirroring(encoded: u8) -> Result<Mirroring, SaveStateError> {
    match encoded {
        0 => Ok(Mirroring::Horizontal),
        1 => Ok(Mirroring::Vertical),
        2 => Ok(Mirroring::FourScreen),
        3 => Ok(Mirroring::SPAGE0),
        4 => Ok(Mirroring::SPAGE1),
        _ => Err(SaveStateError::InvalidData("invalid TQROM mirroring value")),
    }
}
