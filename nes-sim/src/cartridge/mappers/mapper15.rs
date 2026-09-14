use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;

/// K.S.S. multicart board (iNES mapper 015, 100-in-1 style Chinese
/// cartridges). The low bits of the register address pick the PRG layout and
/// bit 7 of the data swaps bank pairs.
pub(super) struct Mapper15 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_banks: [u8; 4],
    mirroring: Mirroring,
}

impl Mapper15 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_rom,
            prg_banks: [0, 1, 2, 3],
            mirroring,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn bank_index(&self, bank: u8) -> usize {
        (bank as usize) % self.prg_bank_count()
    }
}

impl Mapper for Mapper15 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K;
                let bank = self.bank_index(self.prg_banks[slot]);
                Some(self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                let base = (data << 1) & 0xFE;
                let flip = data >> 7;
                self.mirroring = if (data & 0x40) != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
                let paired = |low: u8| base | (low ^ flip);
                match addr & 0x0FFF {
                    0x000 => {
                        self.prg_banks = [paired(0), paired(1), paired(2), paired(3)];
                    }
                    0x001 => {
                        self.prg_banks = [paired(0), paired(1), 0x7E | flip, 0x7F];
                    }
                    0x002 => {
                        let bank = base | flip;
                        self.prg_banks = [bank, bank, bank, bank];
                    }
                    0x003 => {
                        let bank = base | flip;
                        self.prg_banks = [bank, bank.wrapping_add(1), bank, bank.wrapping_add(1)];
                    }
                    _ => {}
                }
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) || self.chr_rom.is_empty() {
            return None;
        }
        Some(self.chr_rom[addr as usize % self.chr_rom.len()])
    }

    fn ppu_write(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_banks);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_banks)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
