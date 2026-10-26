use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// https://www.nesdev.org/wiki/INES_0091
// 未授权Mortal Kombat II / Street Fighter III克隆板。
// $6000-$6FFF写CHR寄存器（4个2K bank），$7000-$7FFF写PRG寄存器（2个8K bank）
// 与IRQ控制；$C000/$E000固定为倒数第二/最后一个8K bank。
// IRQ为8扫描线计数器，使能后第8条扫描线置位，直到再次写入$7002才清除。
const PRG_BANK_LEN: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper91 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    chr_regs: [u8; 4],
    prg_regs: [u8; 2],
    irq_enabled: bool,
    irq_counter: u8,
    irq_active: bool,
    mirroring: Mirroring,
}

impl Mapper91 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            chr_regs: [0; 4],
            prg_regs: [0; 2],
            irq_enabled: false,
            irq_counter: 0,
            irq_active: false,
            mirroring,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn prg_bank_number(&self, slot: usize) -> usize {
        let count = self.prg_bank_count();
        match slot {
            0 => self.prg_regs[0] as usize % count,
            1 => self.prg_regs[1] as usize % count,
            2 => count - 2,
            _ => count - 1,
        }
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let slot = ((addr - 0x8000) / PRG_BANK_LEN as u16) as usize;
        self.prg_bank_number(slot) * PRG_BANK_LEN + (addr as usize & 0x1FFF)
    }

    fn chr_index(&self, addr: u16) -> usize {
        let bank = self.chr_regs[addr as usize / 0x0800] as usize;
        match &self.chr {
            ChrMemory::Rom(chr_rom) => {
                bank % (chr_rom.len() / 0x0800).max(1) * 0x0800 + (addr as usize & 0x07FF)
            }
            ChrMemory::Ram(_) => addr as usize,
        }
    }
}

impl Mapper for Mapper91 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x6FFF => {
                self.chr_regs[(addr & 3) as usize] = data;
                true
            }
            0x7000..=0x7FFF => {
                match addr & 3 {
                    0 | 1 => self.prg_regs[(addr & 1) as usize] = data,
                    2 => {
                        self.irq_enabled = false;
                        self.irq_counter = 0;
                        self.irq_active = false;
                    }
                    _ => {
                        self.irq_enabled = true;
                        self.irq_active = false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match (&self.chr, addr) {
            (ChrMemory::Rom(chr_rom), 0x0000..=0x1FFF) => Some(chr_rom[self.chr_index(addr)]),
            (ChrMemory::Ram(chr_ram), 0x0000..=0x1FFF) => Some(chr_ram[self.chr_index(addr)]),
            _ => None,
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x0000..=0x1FFF if matches!(self.chr, ChrMemory::Ram(_)) => {
                let index = self.chr_index(addr);
                if let ChrMemory::Ram(chr_ram) = &mut self.chr {
                    chr_ram[index] = data;
                }
                true
            }
            0x0000..=0x1FFF => true,
            _ => false,
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn notify_scanline(&mut self, _scanline: i16, _rendering_on: bool) {
        if self.irq_enabled && self.irq_counter < 8 {
            self.irq_counter += 1;
            if self.irq_counter >= 8 {
                self.irq_active = true;
            }
        }
    }

    fn irq_line(&self) -> bool {
        self.irq_active
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.chr_regs);
        writer.write_bytes(&self.prg_regs);
        writer.write_bool(self.irq_enabled);
        writer.write_u8(self.irq_counter);
        writer.write_bool(self.irq_active);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.chr_regs)?;
        reader.read_bytes_into(&mut self.prg_regs)?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_counter = reader.read_u8()?;
        self.irq_active = reader.read_bool()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for mapper 91 save state",
                ));
            }
        }
        Ok(())
    }
}
