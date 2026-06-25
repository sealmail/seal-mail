// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--sealmail-gui-entry-smoke") {
        println!("sealmail-gui-entry");
        return;
    }
    if std::env::var("SEALMAIL_RUN_CLI").as_deref() == Ok("1") {
        sealmail_lib::cli::main_entry();
        return;
    }
    sealmail_lib::run()
}
