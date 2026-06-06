use super::Mapper;
use super::mmc3::Mmc3Core;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper115 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    mirroring: Mirroring,
    core: Mmc3Core,
    ex_prg_switch: u8,
    ex_chr_switch: u8,
}

impl Mapper115 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr,
            mirroring,
            core: Mmc3Core::new(),
            ex_prg_switch: 0,
            ex_chr_switch: 0,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_1K).max(1),
        }
    }

    fn prg_bank_8k(&self, slot: usize) -> usize {
        if self.ex_prg_switch & 0x80 != 0 {
            if self.ex_prg_switch & 0x20 != 0 {
                let bank32 = ((self.ex_prg_switch & 0x0F) >> 1) as usize;
                (bank32 * 4 + slot) % self.prg_bank_count_8k()
            } else {
                let bank16 = (self.ex_prg_switch & 0x0F) as usize;
                (bank16 * 2 + (slot & 1)) % self.prg_bank_count_8k()
            }
        } else {
            self.core.prg_bank_number(self.prg_bank_count_8k(), slot)
        }
    }

    fn chr_bank_1k(&self, slot: usize) -> usize {
        let base = self.core.chr_bank_number(self.chr_bank_count_1k(), slot);
        let offset = ((self.ex_chr_switch & 1) as usize) << 8;
        (base + offset) % self.chr_bank_count_1k()
    }
}

impl Mapper for Mapper115 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K;
                let bank = self.prg_bank_8k(slot);
                let offset = bank * PRG_BANK_8K + ((addr as usize) & 0x1FFF);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000 => {
                self.ex_prg_switch = data;
                true
            }
            0x6001 => {
                self.ex_chr_switch = data & 1;
                true
            }
            0x6002..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = data;
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
        let bank = self.chr_bank_1k(slot);
        let offset = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        match &mut self.chr {
            ChrMemory::Rom(r) => Some(r[offset % r.len()]),
            ChrMemory::Ram(r) => Some(r[offset % r.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = addr as usize / CHR_BANK_1K;
        let bank = self.chr_bank_1k(slot);
        let offset = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        if let ChrMemory::Ram(r) = &mut self.chr {
            let len = r.len();
            r[offset % len] = data;
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
        writer.write_u8(self.ex_prg_switch);
        writer.write_u8(self.ex_chr_switch);
        writer.write_bytes(&self.prg_ram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(r) => {
                writer.write_bool(true);
                writer.write_bytes(r);
            }
        }
        writer.write_u8(encode_mirroring(self.mirroring));
        self.core.save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.ex_prg_switch = reader.read_u8()?;
        self.ex_chr_switch = reader.read_u8()?;
        reader.read_bytes_into(&mut self.prg_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(r), true) => reader.read_bytes_into(r)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Mapper 115 save state",
                ));
            }
        }
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.core.load_state(reader)?;
        Ok(())
    }
}
