//! Feeds `memory.x` to the linker, pulls in cortex-m-rt's `link.x`, and
//! bakes the WiFi credentials from `.env` into the firmware.

use std::{env, fs, path::PathBuf};

fn main() {
    // Linker arguments for every binary we build
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");

    // Copy memory.x next to link.x and put that directory on the search path
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    let memory_x = fs::read_to_string("memory.x").unwrap();
    fs::write(out_dir.join("memory.x"), memory_x).unwrap();
    println!("cargo:rerun-if-changed=memory.x");

    // WiFi credentials: read `.env` (KEY=VALUE lines, # comments) and
    // expose each key as a compile-time environment variable, so
    // `env!("WIFI_SSID")` in main.rs embeds the value into the binary.
    println!("cargo:rerun-if-changed=.env");
    let env_file =
        fs::read_to_string(".env").expect("missing .env - copy .env.example and fill it in");
    for line in env_file.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            println!("cargo:rustc-env={}={}", key.trim(), value);
        }
    }
}
