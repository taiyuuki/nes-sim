use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// BMC 72-in-1 family (iNES mapper 225). The write address latches the
/// mirroring bit, a 16/32 KiB PRG mode bit, the PRG bank and the CHR bank.
pub(super) struct Mapper225 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    mode_16k: bool,
    prg_bank: usize,
    chr_bank: usize,
    mirroring: Mirroring,
}

impl Mapper225 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; crate::cartridge::CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            mode_16k: false,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn prg_bank_count_32k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_32K).max(1)
    }

    fn chr_bank_count(&self) -> usize {
        let len = match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len(),
            ChrMemory::Ram(chr_ram) => chr_ram.len(),
        };
        (len / CHR_BANK_8K).max(1)
    }
}

impl Mapper for Mapper225 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                if self.mode_16k {
                    let bank = self.prg_bank % self.prg_bank_count_16k();
                    Some(
                        self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0x8000) % PRG_BANK_16K],
                    )
                } else {
                    let bank = self.prg_bank % self.prg_bank_count_32k();
                    Some(self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)])
                }
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, _data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.mirroring = if addr & 0x2000 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
                self.mode_16k = addr & 0x1000 != 0;
                let mut bank = (addr as usize >> 7) & 0x1F;
                if self.mode_16k {
                    bank = (bank << 1) | (usize::from(addr >> 6 & 0x1));
                }
                self.prg_bank = bank;
                self.chr_bank = addr as usize;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let offset = (self.chr_bank % self.chr_bank_count()) * CHR_BANK_8K + addr as usize;
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let offset = (self.chr_bank % self.chr_bank_count()) * CHR_BANK_8K + addr as usize;
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[offset] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.mode_16k);
        writer.write_u16(self.prg_bank as u16);
        writer.write_u16(self.chr_bank as u16);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.mode_16k = reader.read_bool()?;
        self.prg_bank = reader.read_u16()? as usize;
        self.chr_bank = reader.read_u16()? as usize;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
