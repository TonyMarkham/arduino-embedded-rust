# Embedded Rust on the Arduino UNO R4 WiFi

A learning project — *you* run every step and type every file; the docs explain
what each thing does and why.

**Mission:** POC firmware that receives a **data packet over WiFi** and
displays a **pattern on the LED matrix**. The repo is a cargo workspace so a
shared protocol crate can serve both the embedded firmware and host-side
tooling:

- `crates/arduino-embedded-rust` — firmware (Renesas RA4M1, `thumbv7em-none-eabihf`)
- `crates/protocol` (planned) — packet format, `no_std`-compatible, shared with the host
- `crates/host-tool` (planned, optional) — PC sender for testing

## The course, in three documents

| Doc | Contents |
|---|---|
| [**Basics.md**](Basics.md) | The board's two chips, the bootloader flash layout, the reset button's two behaviors, toolchain install (Part 0), Troubleshooting + the DFU escape hatch |
| [**BlinkSketch.md**](BlinkSketch.md) | Part 1: prove the USB → bootloader → flash pipeline with a C++ `BlinkWipe` sketch you write, compile, upload, and watch over serial |
| [**BlinkEmbedded.md**](BlinkEmbedded.md) | Parts 2–6: the five files of a bare-metal Rust project (workspace manifests, `.cargo/config.toml`, `memory.x`, `build.rs`, `src/main.rs`), build, convert, flash, verify |

Work in order: Basics → BlinkSketch → BlinkEmbedded. Part/step numbering is
continuous across the docs.

## Status (verified during the first run)

- ✅ Part 0–1: board bring-up, C++ sketch round-trip, serial monitor
- ✅ Parts 2–5: Rust workspace builds (`cargo build --release`), firmware is
  **1,584 bytes** vs the 52 KB C++ baseline, upload + blink verified on
  hardware — first real-hardware validation of the `arduino-uno-r4-hal`
  crate's clock init, GPIO, and delay
- ⏳ Next: the roadmap below, toward the mission

## Roadmap

Each item is an exercise you build in `src/main.rs` (or a new crate) using the
HAL's docs (https://docs.rs/arduino-uno-r4-hal):

1. **Serial output in Rust** — `Serial::new(p.sci2, ...)` on pins D0/D1, then
   `arduino-cli monitor -p $port`. Your first debugging window.
2. **Button input** — D2 as pull-up input, edge detection.
3. **PWM breathing LED** — `pwm::PwmD6`, duty cycles and timers.
4. **Analog reading** — `adc::Adc` on A0.
5. **Interrupts** — the ICU/IELSR event system; the big mental shift.
6. **Bare-metal mode** — drop the HAL, drive registers via the `ra4m1` PAC.
   Background reading: Dom Wilson's series,
   https://domwil.co.uk/posts/uno-r4-rust/ (the basis of this course's setup).
7. **Protocol crate** — `crates/protocol`: the packet format, `no_std`
   compatible, tested on the host without hardware.
8. **LED matrix driver** — first verify who drives the 12×8 matrix (RA4M1 vs
   ESP32-S3) from the `Arduino_LED_Matrix` source and the board schematic;
   then port the driver to Rust.
9. **WiFi** — implement the ESP32-S3 bridge protocol subset in Rust over UART
   (join AP, receive packets), ported from `WiFiS3` / `uno-r4-wifi-usb-bridge`
   source. Then: receive a packet, decode it, render the pattern. Mission
   complete.

## Honest caveats

- The `arduino-uno-r4-hal` crate is young (v0.2), originally published as
  unverified on hardware. During this course's first run it **was verified on
  a real UNO R4 WiFi** for: 48 MHz clock init, D13 GPIO output, and SysTick
  delay (the Part 6 blinky). Not yet exercised on hardware: UART, ADC, PWM —
  treat those as first-test territory.
- Rust on the R4's ESP32-S3 (the WiFi chip) is a separate, harder problem —
  the WiFi lesson depends on porting Arduino's bridge protocol; budget for it.
- Agent rules for this repo live in [AGENTS.md](AGENTS.md): the owner runs
  every step; agents write and maintain the docs only.
