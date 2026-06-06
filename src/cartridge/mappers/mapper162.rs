use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_32K: usize = 0x8000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper162 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    reg: [u8; 4],
    mirroring: Mirroring,
}

impl Mapper162 {
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
            reg: [3, 0, 0, 7],
            mirroring,
        }
    }

    fn prg_bank(&self) -> usize {
        let mode = self.reg[3] & 7;
        let bank = match mode {
            0 | 2 => {
                (self.reg[0] & 0x0C) as usize
                    | (self.reg[1] & 0x02) as usize
                    | ((self.reg[2] & 0x0F) as usize) << 4
            }
            1 | 3 => (self.reg[0] & 0x0C) as usize | ((self.reg[2] & 0x0F) as usize) << 4,
            4 | 6 => {
                (self.reg[0] & 0x0E) as usize
                    | ((self.reg[1] >> 1) & 1) as usize
                    | ((self.reg[2] & 0x0F) as usize) << 4
            }
            _ => (self.reg[0] & 0x0F) as usize | ((self.reg[2] & 0x0F) as usize) << 4,
        };
        (bank * PRG_BANK_32K) % self.prg_rom.len()
    }
}

impl Mapper for Mapper162 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => {
                let base = self.prg_bank();
                let offset = base + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x5000..=0x5FFF => {
                self.reg[((addr >> 8) & 3) as usize] = data;
                true
            }
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        match &mut self.chr {
            ChrMemory::Rom(r) => Some(r[addr as usize % r.len()]),
            ChrMemory::Ram(r) => Some(r[addr as usize % r.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        if let ChrMemory::Ram(r) = &mut self.chr {
            let len = r.len();
            r[addr as usize % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.reg);
        writer.write_bytes(&self.prg_ram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(r) => {
                writer.write_bool(true);
                writer.write_bytes(r);
            }
        }
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.reg)?;
        reader.read_bytes_into(&mut self.prg_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(r), true) => reader.read_bytes_into(r)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Mapper 162 save state",
                ));
            }
        }
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
