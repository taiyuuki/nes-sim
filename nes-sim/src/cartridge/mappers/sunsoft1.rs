use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_4K: usize = 0x1000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

fn new_chr(chr_rom: Vec<u8>) -> ChrMemory {
    if chr_rom.is_empty() {
        ChrMemory::Ram(vec![0; CHR_BANK_8K])
    } else {
        ChrMemory::Rom(chr_rom)
    }
}

/// Sunsoft-1 board (iNES mapper 093, Fantasy Zone / Shanghai).
pub(super) struct Sunsoft1 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    prg_bank: u8,
    mirroring: Mirroring,
}

impl Sunsoft1 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr: new_chr(chr_rom),
            prg_bank: 0,
            mirroring,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }
}

impl Mapper for Sunsoft1 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = ((self.prg_bank >> 4) & 0x0F) as usize % self.prg_bank_count_16k();
                Some(self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0x8000)])
            }
            0xC000..=0xFFFF => {
                let last = self.prg_bank_count_16k() - 1;
                Some(self.prg_rom[last * PRG_BANK_16K + (addr as usize - 0xC000)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.prg_bank = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[addr as usize % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[addr as usize]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[addr as usize] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        save_chr_state(&self.chr, writer);
        writer.write_u8(self.prg_bank);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        load_chr_state(&mut self.chr, reader)?;
        self.prg_bank = reader.read_u8()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}

/// Sunsoft-1 CHR-switching variant (iNES mapper 184, The Wing of Madoola).
pub(super) struct Sunsoft184 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    chr_banks: [u8; 2],
    mirroring: Mirroring,
}

impl Sunsoft184 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr: new_chr(chr_rom),
            chr_banks: [0, 0],
            mirroring,
        }
    }

    fn chr_bank_count_4k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_4K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_4K).max(1),
        }
    }
}

impl Mapper for Sunsoft184 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[addr as usize - 0x8000]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                self.chr_banks[0] = data & 0x07;
                self.chr_banks[1] = (data >> 4) & 0x07;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let slot = addr as usize / CHR_BANK_4K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_4k();
        let offset = bank * CHR_BANK_4K + (addr as usize & 0x0FFF);
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = addr as usize / CHR_BANK_4K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_4k();
        let offset = bank * CHR_BANK_4K + (addr as usize & 0x0FFF);
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            let len = chr_ram.len();
            chr_ram[offset % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        save_chr_state(&self.chr, writer);
        writer.write_bytes(&self.chr_banks);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        load_chr_state(&mut self.chr, reader)?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}

/// Sunsoft-1 with CHR copy protection (iNES mapper 185, B-Wings / Mighty
/// Bomb Jack). While the protection trips, CHR reads return open-bus style
/// garbage so the game detects an unauthorized cartridge.
pub(super) struct Sunsoft185 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    chr_bank: u8,
    chr_enabled: bool,
    mirroring: Mirroring,
}

impl Sunsoft185 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr: new_chr(chr_rom),
            chr_bank: 0,
            chr_enabled: true,
            mirroring,
        }
    }

    fn chr_bank_count_8k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_8K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_8K).max(1),
        }
    }
}

impl Mapper for Sunsoft185 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[addr as usize - 0x8000]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.chr_bank = data;
                self.chr_enabled = (data & 0x03) != 0 && data != 0x13;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        if !self.chr_enabled {
            return Some(0xFF);
        }
        let bank = self.chr_bank as usize % self.chr_bank_count_8k();
        let offset = bank * CHR_BANK_8K + addr as usize;
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        if let (true, ChrMemory::Ram(chr_ram)) = (self.chr_enabled, &mut self.chr) {
            chr_ram[addr as usize] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        save_chr_state(&self.chr, writer);
        writer.write_u8(self.chr_bank);
        writer.write_bool(self.chr_enabled);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        load_chr_state(&mut self.chr, reader)?;
        self.chr_bank = reader.read_u8()?;
        self.chr_enabled = reader.read_bool()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}

fn save_chr_state(chr: &ChrMemory, writer: &mut StateWriter) {
    match chr {
        ChrMemory::Rom(_) => writer.write_bool(false),
        ChrMemory::Ram(chr_ram) => {
            writer.write_bool(true);
            writer.write_bytes(chr_ram);
        }
    }
}

fn load_chr_state(chr: &mut ChrMemory, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
    let has_chr_ram = reader.read_bool()?;
    match (chr, has_chr_ram) {
        (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
        (ChrMemory::Rom(_), false) => {}
        _ => {
            return Err(SaveStateError::InvalidData(
                "CHR RAM mismatch for Sunsoft-1 save state",
            ));
        }
    }
    Ok(())
}
