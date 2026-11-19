//! Generates the Tauri glue code from `tauri.conf.json` and the capability files.

fn main() {
    tauri_build::build();
}
