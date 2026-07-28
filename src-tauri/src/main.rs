// Hide the console window in release builds (Windows GUI subsystem).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    vrchat_photo_manager_lib::run()
}
