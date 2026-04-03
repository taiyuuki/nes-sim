use super::Mapper;
use super::mmc3::Mmc3Core;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_LEN: usize = 0x2000;
const CHR_BANK_LEN_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper118 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    mirroring: Mirroring,
    core: Mmc3Core,
}

impl Mapper118 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr,
            mirroring,
            core: Mmc3Core::new(),
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_LEN
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len() / CHR_BANK_LEN_1K,
            ChrMemory::Ram(chr_ram) => chr_ram.len() / CHR_BANK_LEN_1K,
        }
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let slot = ((addr - 0x8000) as usize) / PRG_BANK_LEN;
        let bank = self.core.prg_bank_number(self.prg_bank_count(), slot);
        bank * PRG_BANK_LEN + ((addr as usize) & 0x1FFF)
    }

    fn chr_index(&self, addr: u16) -> usize {
        let slot = (addr as usize) / CHR_BANK_LEN_1K;
        let bank =
            usize::from(self.core.effective_chr_bank_value(slot) & 0x7F) % self.chr_bank_count_1k();
        bank * CHR_BANK_LEN_1K + ((addr as usize) & 0x03FF)
    }
}

impl Mapper for Mapper118 {
    fn mapper_id(&self) -> u16 {
        118
    }

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
            0xA000..=0xBFFF if (addr & 1) == 0 => true,
            0x8000..=0xFFFF => self.core.write_register(addr, data, None),
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let index = self.chr_index(addr);
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[index]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[index]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let index = self.chr_index(addr);
        match &mut self.chr {
            ChrMemory::Ram(chr_ram) => {
                chr_ram[index] = data;
                true
            }
            ChrMemory::Rom(_) => true,
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn map_nametable_addr(&self, addr: u16) -> Option<usize> {
        if !matches!(addr, 0x2000..=0x3EFF) {
            return None;
        }

        let offset = (addr - 0x2000) & 0x0FFF;
        let slot = (offset >> 10) as usize;
        let inner = (offset & 0x03FF) as usize;
        let ciram_page = usize::from(self.core.effective_chr_bank_value(slot) >> 7);
        Some(ciram_page * 0x0400 + inner)
    }

    fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        self.core.check_a12(addr, ppu_cycle);
    }

    fn irq_line(&self) -> bool {
        self.core.irq_line()
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        writer.write_u8(encode_mirroring(self.mirroring));
        self.core.save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for MMC118 save state",
                ));
            }
        }
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.core.load_state(reader)
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
        _ => Err(SaveStateError::InvalidData(
            "invalid MMC118 mirroring value",
        )),
    }
}
