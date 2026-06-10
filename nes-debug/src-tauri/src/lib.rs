mod emulator;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            emulator::load_rom,
            emulator::reset,
            emulator::step_frame,
            emulator::step_instruction,
            emulator::run_frame,
            emulator::toggle_pause,
            emulator::get_debug_info,
            emulator::get_frame,
            emulator::read_ram,
            emulator::read_vram,
            emulator::read_chr,
            emulator::read_oam,
            emulator::read_palette,
            emulator::add_breakpoint,
            emulator::remove_breakpoint,
            emulator::set_paused,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
