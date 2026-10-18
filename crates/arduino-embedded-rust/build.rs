//! Feeds `memory.x` to the linker and pulls in cortex-m-rt's `link.x`.

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
}
