use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// BMC 76-in-1 (iNES mapper 226). Two 8-bit latches selected by address
/// bit 0 combine into the 16/32 KiB PRG bank number and a mirroring bit;
/// the 8 KiB CHR window is fixed.
pub(super) struct Mapper226 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    regs: [u8; 2],
    mirroring: Mirroring,
}

impl Mapper226 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; crate::cartridge::CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            regs: [0, 0],
            mirroring,
        }
    }

    fn bank_32k(&self) -> usize {
        ((self.regs[0] as usize) >> 1 & 0x0F)
            | ((self.regs[0] as usize) >> 3 & 0x10)
            | ((self.regs[1] as usize) << 5 & 0x20)
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn prg_bank_count_32k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_32K).max(1)
    }
}

impl Mapper for Mapper226 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                if self.regs[0] & 0x20 != 0 {
                    let bank = ((self.bank_32k() << 1) | (self.regs[0] & 0x1) as usize)
                        % self.prg_bank_count_16k();
                    Some(
                        self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0x8000) % PRG_BANK_16K],
                    )
                } else {
                    let bank = self.bank_32k() % self.prg_bank_count_32k();
                    Some(self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)])
                }
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.regs[(addr & 0x1) as usize] = data;
                self.mirroring = if self.regs[0] & 0x40 != 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
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
        writer.write_bytes(&self.regs);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.regs)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
