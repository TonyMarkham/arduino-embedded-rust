# BlinkEmbedded — Parts 2–6: write, build, flash, and verify bare-metal Rust

This is the Rust half of the course. Every file you type from here on is Rust.

Prerequisites:
- [Basics.md](Basics.md) — toolchain installed (Part 0), reset-button behaviors,
  Troubleshooting + DFU escape hatch live there.
- [BlinkSketch.md](BlinkSketch.md) — Part 1 complete: board brings-up, and
  `$port` is captured in your terminal session.

## Part 2 — Create the Rust project

> **Step 2.1 — Initialize the cargo package**
>
> ```powershell
> cargo init --name arduino-embedded-rust
> ```
>
> If you prefer a workspace layout (recommended when a shared protocol crate
> is planned — see README), create the workspace root and a member package
> instead; Step 3.1 describes the resulting two manifests.

> **Step 2.2 — Inspect what cargo made**
>
> ```powershell
> Get-ChildItem -Force
> ```

**What/why:** cargo created `Cargo.toml` (package manifest) and `src/main.rs`
(a hello-world). We're going to replace both, plus add four more files.

✅ **Checkpoint:** you can see `Cargo.toml` and `src/` in the listing.

---

## Part 3 — The five files, one by one

> **Step 3.1 — The two manifests (workspace layout)**
>
> This project is a cargo workspace: the **root manifest** holds shared
> dependency definitions; the **package manifest** (in `crates\arduino-embedded-rust\`)
> inherits them and defines the firmware itself. Both already exist from
> `cargo init` — replace their contents with:
>
> Root `Cargo.toml` — shared deps, the build profiles (in a workspace,
> `[profile.*]` is only honored at the workspace root), and the lints tables:
>
> ```toml
> [workspace]
> members            = [
>     "crates/arduino-embedded-rust",
> ]
> resolver           = "3"
>
> [workspace.package]
> version            = "0.1.0"
> edition            = "2024"
>
> [workspace.dependencies]
> # HAL for the UNO R4 — the WiFi board needs the `uno-r4-wifi` feature
> # (its header pin mapping differs from the Minima board).
> arduino-uno-r4-hal = { version = "0.2.0", default-features = false, features = ["uno-r4-wifi"] }
> # Runtime: startup code, interrupt vector table, the #[entry] attribute
> cortex-m-rt        = { version = "0.7.7", features = ["set-vtor"] }
> # The trait collection shared by all embedded Rust crates (DelayNs, etc.)
> embedded-hal       = { version = "1.0.0" }
> # On panic: halt the core. Simplest choice with no debugger attached.
> panic-halt         = { version = "1.0.0" }
>
> [workspace.lints.clippy]
> unwrap_used        = "deny"
> expect_used        = "deny"
> panic              = "deny"
>
> [workspace.lints.rust]
> unused_must_use    = "deny"
>
> [profile.release]
> opt-level          = 3    # speed matters on a 48 MHz chip
> lto                = true # link-time optimization: smaller + faster firmware
> codegen-units      = 1    # more optimization, slower build — fine for firmware
> debug              = true # keep symbols so cargo size / backtraces are readable
> ```
>
> No `[profile.dev]` section: dev builds use cargo defaults, which also keeps
> the future host-tool crate's dev builds standard. If the firmware's dev
> build ever grows toward the flash limit, scope optimization to it alone
> with a per-package override in the root manifest —
> `[profile.dev.package.arduino-embedded-rust]` with `opt-level = 1` — rather
> than de-optimizing every crate in the workspace.
>
> Package `crates\arduino-embedded-rust\Cargo.toml` — no `[profile.*]`
> sections here (they'd be ignored with a warning):
>
> ```toml
> [package]
> name               = "arduino-embedded-rust"
> version            = { workspace = true }
> edition            = { workspace = true }
>
> [dependencies]
> arduino-uno-r4-hal = { workspace = true }
> cortex-m-rt        = { workspace = true }
> embedded-hal       = { workspace = true }
> panic-halt         = { workspace = true }
> ```
>
> Note: do **not** add `[lints] workspace = true` to the package. The
> workspace lints deny `unwrap`/`panic`, but `Peripherals::take().unwrap()`
> is idiomatic embedded — the panic handler (`panic-halt`) is the failure
> strategy on a microcontroller.

> **Step 3.2 — Write `crates\arduino-embedded-rust\.cargo\config.toml` (the cross-compile target)**
>
> Create the folder `.cargo` **inside the package directory**
> (`crates\arduino-embedded-rust\.cargo\`) and inside it a file `config.toml`:
>
> ```toml
> [build]
> # The RA4M1 is an ARM Cortex-M4F (ARMv7E-M with a hardware FPU)
> target = "thumbv7em-none-eabihf"
> ```
>
> There is deliberately **no** `.cargo/` at the workspace root. Cargo applies
> config by directory: building from inside the package pins the Cortex-M
> target, while builds from the workspace root (the future host-tool crate)
> stay native. **All firmware cargo commands in this doc therefore run from
> `crates\arduino-embedded-rust\`.** The `target\` output directory still
> lands at the workspace root.

**What/why:** this makes plain `cargo build` from the package directory
automatically cross-compile — you never have to type `--target`. Note what is
*not* here: no `-Tlink.x` rustflags. The linker arguments are passed by
`build.rs` (Step 3.4) — linker args must have exactly **one** source. When a
config file *and* `build.rs` both pass `-Tlink.x`, the linker script gets read
twice, `memory.x` gets included twice, and the build dies with
`region 'FLASH' already defined`. (Confirmed the hard way during this
course's first run.)

> **Step 3.3 — Write `memory.x` (the memory map — the part that keeps your board alive)**
>
> Create `memory.x` in the **package directory** `crates\arduino-embedded-rust\`
> (next to its `Cargo.toml` — `build.rs` will read it relative to there):
>
> ```
> /* Memory layout for the Arduino UNO R4 WiFi (Renesas RA4M1: R7FA4M1AB3CFM).
>  *
>  * The Arduino bootloader occupies the first 16 KiB of flash and jumps to
>  * our code at 0x4000. Keeping it intact lets us flash over plain USB.
>  *
>  * DON'T set FLASH ORIGIN to 0x00000000 — that erases the bootloader.
>  */
> MEMORY
> {
>   FLASH : ORIGIN = 0x00004000, LENGTH = 240K
>   RAM   : ORIGIN = 0x20000000, LENGTH = 32K
> }
> ```

**What/why:** embedded programming starts with the datasheet. The RA4M1's flash
is mapped at `0x00000000` and RAM at `0x20000000` (Cortex-M convention). We
start our code 16 KiB in (`0x4000`) and shrink the available flash to 240 K so
the linker can never place code over the bootloader.

> **Step 3.4 — Write `build.rs` (glue for the linker)**
>
> Create `build.rs` in the **package directory** `crates\arduino-embedded-rust\`:
>
> ```rust
> //! Feeds `memory.x` to the linker and pulls in cortex-m-rt's `link.x`.
>
> use std::{env, fs, path::PathBuf};
>
> fn main() {
>     // Linker arguments for every binary we build
>     println!("cargo:rustc-link-arg-bins=--nmagic");
>     println!("cargo:rustc-link-arg-bins=-Tlink.x");
>
>     // Copy memory.x next to link.x and put that directory on the search path
>     let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
>     println!("cargo:rustc-link-search=native={}", out_dir.display());
>     let memory_x = fs::read_to_string("memory.x").unwrap();
>     fs::write(out_dir.join("memory.x"), memory_x).unwrap();
>     println!("cargo:rerun-if-changed=memory.x");
> }
> ```

**What/why:** a `build.rs` runs on the *host* before compilation. `link.x`
(cortex-m-rt's script) contains `INCLUDE memory.x` — so the linker needs our
`memory.x` on its search path. `--nmagic` disables section padding so the
firmware image is compact. `rerun-if-changed` makes cargo rebuild when you edit
the memory map.

> **Step 3.5 — Write `src/main.rs` (the blinky)**
>
> Replace `crates\arduino-embedded-rust\src\main.rs`:
>
> ```rust
> //! Lesson 1: blink the onboard LED (pin D13) at 1 Hz.
>
> #![no_std]   // don't link the standard library — no OS down here
> #![no_main]  // no standard main(); we provide our own entry point
>
> use arduino_uno_r4_hal::Peripherals;
> use embedded_hal::delay::DelayNs;
> use panic_halt as _; // crash handler: halt the core on panic
>
> // `#[entry]` becomes the function the vector table points to.
> // It can never return (`-> !`) — there is nothing to return *to*.
> #[cortex_m_rt::entry]
> fn main() -> ! {
>     // Claim all peripherals exactly once; also initializes the 48 MHz clock.
>     let p = Peripherals::take().unwrap();
>
>     let mut delay = p.delay;                 // SysTick-based blocking delay
>     let mut led = p.pins.d13.into_output();  // D13 = onboard LED
>
>     loop {
>         led.toggle();
>         delay.delay_ms(500);
>     }
> }
> ```

**What/why:**

- `#![no_std]` / `#![no_main]` are the two markers of every bare-metal program.
- `Peripherals::take()` is the embedded-Rust ownership trick: the peripherals
  struct can be taken **once**; after that you get `None`. This makes it a
  compile error for two pieces of code to drive the same register.
- The pin is a *type-state* object: `d13.into_output()` consumes the input-mode
  pin and returns an output-mode pin. You literally cannot call `toggle()` on a
  pin the compiler believes is an input.
- The LED on the WiFi board is wired to Arduino pin D13 (RA4M1 port P102).

✅ **Checkpoint:** `Get-ChildItem` at the workspace root shows `Cargo.toml`
(there is deliberately **no** `.cargo/` there) — and
`Get-ChildItem crates\arduino-embedded-rust -Force` shows `Cargo.toml`,
`.cargo\`, `build.rs`, `memory.x`, `src\main.rs`.

---

## Part 4 — Build

> **Step 4.1 — Compile in release mode**
>
> All firmware cargo commands run from inside the package directory (the
> target pin lives in its `.cargo/config.toml` — see Step 3.2):
>
> ```powershell
> Set-Location crates\arduino-embedded-rust
> cargo build --release
> ```
>
> First build takes a couple of minutes (the HAL compiles from source).

> **Step 4.2 — Inspect the firmware size**
>
> Still inside `crates\arduino-embedded-rust\`:
>
> ```powershell
> cargo size --release
> ```

**What/why:** `size` shows text (flash used), data (initialized globals copied
to RAM), and bss (zeroed RAM). Compare against the C++ baseline from Step 1.3b
(~52 KB): the Rust build should be far smaller, because the HAL links only
what you call. Verified in this course's first run: **1,584 bytes** — a 33×
reduction.

✅ **Checkpoint:** build succeeds. If not, see Troubleshooting in Basics.md.

---

## Part 5 — Convert and flash

> **Step 5.1 — Convert ELF → raw binary**
>
> The compiler produces an ELF file — a container holding the loadable
> sections *plus* symbol tables and (since `debug = true`) DWARF debug
> sections. The bootloader wants only the loadable parts as a flat image.
> cargo-binutils injects the built ELF automatically, so the single positional
> argument is the **output** file:
>
> ```powershell
> cargo objcopy --release -- -O binary firmware.bin
> ```
>
> Output lands in the package directory; the ELF it reads is at the
> workspace-root `target\` (relative from here: `..\..\target\`). Expected
> result (verified in this course): an ELF of several hundred KB reduced to
> ~1.6 KB — your 1,584 bytes of `text`, page-aligned:
>
> ```powershell
> Get-Item ..\..\target\thumbv7em-none-eabihf\release\arduino-embedded-rust, firmware.bin |
>   Select-Object Name, Length
> ```

**What/why:** because of our memory map, byte 0 of this file lands at flash
address `0x4000` — right where the bootloader expects your sketch, and safely
above the 16 KiB described in Basics.md.

> **Step 5.2 — Confirm the port is still there**
>
> ```powershell
> arduino-cli board list
> ```
>
> If the COM number changed, re-set `$port` (captured in BlinkSketch.md
> Step 1.2).

> **Step 5.3 — Upload the firmware**
>
> ⚠ **Never upload cold** — double-tap reset (LED fades = bootloader mode),
> re-check the port with `arduino-cli board list`, re-set `$port` if it
> changed, then (from the package directory, where `firmware.bin` lives):
>
> ```powershell
> arduino-cli upload -p $port --fqbn arduino:renesas_uno:unor4wifi --input-file firmware.bin
> ```
>
> This erases the BlinkWipe sketch and writes your Rust firmware — the
> round-trip you proved in Part 1, now with your own bare-metal binary.
> Repeated mid-write `SAM-BA operation failed` → **DFU escape hatch** in
> Basics.md.

✅ **Checkpoint:** upload reports 100%.

---

## Part 6 — Verify

> **Step 6.1 — Watch the LED**
>
> The onboard LED should blink once per second. If it does: you just wrote,
> linked, and flashed a bare-metal ARM program with no OS, no bootloader
> magic, and no C. 🎉 Verified in this course's first run — the HAL's clock
> init, GPIO, and delay all work on real hardware.

---

Next: the roadmap in [README](README.md) — toward the project goal: receiving
a data packet over WiFi and displaying a pattern on the LED matrix.
