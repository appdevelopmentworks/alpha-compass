// Prevents an additional console window on Windows in release; ignored in dev.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    alpha_compass_lib::run();
}
