#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod app;
mod auth;
mod client;
mod download;
#[cfg(test)]
mod e2e;
mod game;
mod install;
mod mc;
mod net;
mod paths;
mod skin;
mod store;
mod sys;

fn main() {
    if let Err(e) = app::run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
