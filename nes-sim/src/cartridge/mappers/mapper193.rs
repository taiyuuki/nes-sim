use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// https://www.nesdev.org/wiki/INES_0193
// NTDEC Mega Soft Fighting Hero：$6000-$6003写入四个寄存器。
// PRG：$8000为8K可切换（reg[3]），$A000/$C000/$E000固定为0x0D/0x0E/0x0F；
// CHR：$0000为4K（reg[0]>>2），$1000/$1800为2K（reg[1]>>1、reg[2]>>1）。
// 镜像固定垂直。
const PRG_BANK_LEN: usize = 0x2000;
const CHR_BANK_LEN_2K: usize = 0x0800;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper193 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    regs: [u8; 4],
}

impl Mapper193 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, _mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            regs: [0; 4],
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn prg_bank_number(&self, slot: usize) -> usize {
        match slot {
            0 => self.regs[3] as usize % self.prg_bank_count(),
            1 => 0x0D % self.prg_bank_count(),
            2 => 0x0E % self.prg_bank_count(),
            _ => 0x0F % self.prg_bank_count(),
        }
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let slot = ((addr - 0x8000) / PRG_BANK_LEN as u16) as usize;
        self.prg_bank_number(slot) * PRG_BANK_LEN + (addr as usize & 0x1FFF)
    }

    fn chr_rom_index(&self, addr: u16) -> usize {
        let bank = if addr < 0x1000 {
            (self.regs[0] >> 2) as usize
        } else {
            (self.regs[(addr as usize / 0x0800) - 1] >> 1) as usize
        };
        let chr_len = match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len(),
            ChrMemory::Ram(_) => CHR_BANK_LEN,
        };
        bank % (chr_len / CHR_BANK_LEN_2K).max(1) * CHR_BANK_LEN_2K + (addr as usize & 0x07FF)
    }
}

impl Mapper for Mapper193 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x6003 => {
                self.regs[(addr & 3) as usize] = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match (&self.chr, addr) {
            (ChrMemory::Rom(chr_rom), 0x0000..=0x1FFF) => Some(chr_rom[self.chr_rom_index(addr)]),
            (ChrMemory::Ram(chr_ram), 0x0000..=0x1FFF) => Some(chr_ram[addr as usize]),
            _ => None,
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        match (&mut self.chr, addr) {
            (ChrMemory::Ram(chr_ram), 0x0000..=0x1FFF) => {
                chr_ram[addr as usize] = data;
                true
            }
            (ChrMemory::Rom(_), 0x0000..=0x1FFF) => true,
            _ => false,
        }
    }

    fn mirroring(&self) -> Mirroring {
        Mirroring::Vertical
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.regs);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.regs)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for mapper 193 save state",
                ));
            }
        }
        Ok(())
    }
}
