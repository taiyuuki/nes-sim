use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K_LEN: usize = 0x2000;
const CHR_BANK_2K_LEN: usize = 0x0800;
const CHR_BANK_1K_LEN: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Data East DE1ROM/DOROM (iNES mapper 206, also used for Namco 34xx
/// boards). A reduced MMC3: only the bank-select and bank-data registers
/// exist, the PRG mode / CHR inversion bits are hardwired, the CHR layout
/// is fixed to 2 KiB + 2 KiB + 4 x 1 KiB, the top two 8 KiB PRG banks are
/// fixed, and there is no scanline counter.
pub(super) struct Mapper206 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    bank_select: u8,
    bank_registers: [u8; 8],
    mirroring: Mirroring,
}

impl Mapper206 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; crate::cartridge::CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            bank_select: 0,
            bank_registers: [0, 0, 0, 0, 0, 0, 0, 1],
            mirroring,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K_LEN).max(1)
    }

    fn prg_bank(&self, slot: usize) -> usize {
        let count = self.prg_bank_count_8k();
        match slot {
            0 => usize::from(self.bank_registers[6] & 0x0F) % count,
            1 => usize::from(self.bank_registers[7] & 0x0F) % count,
            2 => count - 2,
            _ => count - 1,
        }
    }

    fn chr_index(&self, addr: u16) -> Option<usize> {
        let offset = addr as usize;
        match addr {
            0x0000..=0x07FF => Some(
                (usize::from(self.bank_registers[0]) % (self.chr_len() / CHR_BANK_2K_LEN).max(1))
                    * CHR_BANK_2K_LEN
                    + offset,
            ),
            0x0800..=0x0FFF => Some(
                (usize::from(self.bank_registers[1]) % (self.chr_len() / CHR_BANK_2K_LEN).max(1))
                    * CHR_BANK_2K_LEN
                    + (offset - 0x0800),
            ),
            0x1000..=0x1FFF => {
                let slot = (offset - 0x1000) / CHR_BANK_1K_LEN;
                Some(
                    (usize::from(self.bank_registers[2 + slot])
                        % (self.chr_len() / CHR_BANK_1K_LEN).max(1))
                        * CHR_BANK_1K_LEN
                        + (offset & 0x03FF),
                )
            }
            _ => None,
        }
    }

    fn chr_len(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len(),
            ChrMemory::Ram(chr_ram) => chr_ram.len(),
        }
        .max(CHR_BANK_1K_LEN)
    }
}

impl Mapper for Mapper206 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K_LEN;
                let index = self.prg_bank(slot) * PRG_BANK_8K_LEN + ((addr as usize) & 0x1FFF);
                Some(self.prg_rom[index])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                if (addr & 1) == 0 {
                    self.bank_select = data & 0x07;
                } else {
                    let index = (self.bank_select & 0x07) as usize;
                    let mut value = if index <= 5 { data & 0x3F } else { data & 0x0F };
                    // Registers 0/1 drive 2 KiB banks from a 4 KiB value.
                    if index <= 1 {
                        value >>= 1;
                    }
                    self.bank_registers[index] = value;
                }
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        let index = self.chr_index(addr)?;
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[index % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[index % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        let index = match self.chr_index(addr) {
            Some(index) => index,
            None => return false,
        };
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            let len = chr_ram.len();
            chr_ram[index % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.bank_select);
        writer.write_bytes(&self.bank_registers);
        writer.write_u8(super::encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.bank_select = reader.read_u8()?;
        reader.read_bytes_into(&mut self.bank_registers)?;
        self.mirroring = super::decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
