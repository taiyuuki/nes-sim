use nes_sim::NES;
use std::env;

fn read(nes: &mut NES, a: u16) -> u8 {
    nes.debug_cpu_read(a)
}

fn main() {
    let path = env::args().nth(1).expect("rom path");
    let rom = std::fs::read(&path).unwrap();
    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).unwrap();
    nes.reset();

    for frame in 0..3600 {
        nes.run_frame();
        if [
            read(&mut nes, 0x6001),
            read(&mut nes, 0x6002),
            read(&mut nes, 0x6003),
        ] != [0xDE, 0xB0, 0x61]
        {
            continue;
        }
        let status = read(&mut nes, 0x6000);
        if status >= 0x80 {
            continue;
        }
        let mut v = Vec::new();
        for i in 0..0x600u16 {
            let b = read(&mut nes, 0x6004 + i);
            if b == 0 {
                break;
            }
            v.push(b);
        }
        println!("status={status:#04x} frame={frame}");
        println!("{}", String::from_utf8_lossy(&v));
        return;
    }
    println!("timeout");
}
