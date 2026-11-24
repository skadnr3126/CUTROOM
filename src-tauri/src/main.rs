// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod codex_registration;

fn main() {
    #[cfg(windows)]
    if let Some(arg) = std::env::args_os().nth(1) {
        if arg == "--register-codex-mcp" || arg == "--unregister-codex-mcp" {
            if let Err(error) = codex_registration::run(arg == "--register-codex-mcp") {
                eprintln!("Cutroom MCP setup failed: {error}");
                std::process::exit(1);
            }
            return;
        }
    }
    tauri_app_lib::run()
}
