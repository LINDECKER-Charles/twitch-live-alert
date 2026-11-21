#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod service;
mod shell;
mod win;

fn main() {
    let background = shell::background();
    let code = shell::run(background).unwrap_or_else(|error| {
        shell::report(&error, background);
        1
    });
    std::process::exit(code);
}
