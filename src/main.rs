//! Ferrocord — client Discord natif (non officiel) en Rust.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    ferrocord::run()
}
