use super::Mapper;
use super::mmc3::Mmc3Core;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_LEN: usize = 0x2000;
const CHR_BANK_LEN_1K: usize = 0x0400;
const CHR_RAM_LEN: usize = 0x0800;

/// Waixing MMC3 derivative (iNES mapper 074, used by Chinese releases such
/// as Metal Max translations). Identical to MMC3 except CHR bank values
/// $08/$09 address 2 KiB of CHR-RAM at PPU $0000-$07FF.
pub(super) struct Mapper74 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_rom: Vec<u8>,
    // With CHR-ROM present this holds the board's extra 2 KiB at banks
    // $08/$09; CHR-RAM-only cartridges instead back the whole 8 KiB PPU
    // window with it.
    chr_ram: Vec<u8>,
    chr_ram_full_window: bool,
    mirroring: Mirroring,
    core: Mmc3Core,
}

impl Mapper74 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr_ram_full_window = chr_rom.is_empty();
        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr_rom,
            chr_ram: if chr_ram_full_window {
                vec![0; 0x2000]
            } else {
                vec![0; CHR_RAM_LEN]
            },
            chr_ram_full_window,
            mirroring,
            core: Mmc3Core::new(),
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr_rom.len() / CHR_BANK_LEN_1K).max(1)
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let slot = ((addr - 0x8000) as usize) / PRG_BANK_LEN;
        let bank = self.core.prg_bank_number(self.prg_bank_count(), slot);
        bank * PRG_BANK_LEN + ((addr as usize) & 0x1FFF)
    }

    fn chr_bank_value(&self, addr: u16) -> u8 {
        let slot = (addr as usize) / CHR_BANK_LEN_1K;
        self.core.effective_chr_bank_value(slot)
    }
}

impl Mapper for Mapper74 {
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
        if self.chr_ram_full_window {
            return Some(self.chr_ram[addr as usize]);
        }
        let bank_value = self.chr_bank_value(addr);
        if matches!(bank_value, 8 | 9) {
            let offset = (bank_value as usize - 8) * CHR_BANK_LEN_1K + (addr as usize & 0x03FF);
            return Some(self.chr_ram[offset]);
        }
        let bank = (bank_value as usize) % self.chr_bank_count_1k();
        Some(self.chr_rom[bank * CHR_BANK_LEN_1K + (addr as usize & 0x03FF)])
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        if self.chr_ram_full_window {
            self.chr_ram[addr as usize] = data;
            return true;
        }
        let bank_value = self.chr_bank_value(addr);
        if matches!(bank_value, 8 | 9) {
            let offset = (bank_value as usize - 8) * CHR_BANK_LEN_1K + (addr as usize & 0x03FF);
            self.chr_ram[offset] = data;
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
        writer.write_bool(self.chr_ram_full_window);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        reader.read_bytes_into(&mut self.chr_ram)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.core.load_state(reader)?;
        let _ = reader.read_bool()?;
        Ok(())
    }
}
