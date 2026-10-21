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

## The course, in six documents

| Doc | Contents |
|---|---|
| [**Basics.md**](Basics.md) | The board's two chips, the bootloader flash layout, the reset button's two behaviors, toolchain install (Part 0), Troubleshooting + the DFU escape hatch |
| [**BlinkSketch.md**](BlinkSketch.md) | Part 1: prove the USB → bootloader → flash pipeline with a C++ `BlinkWipe` sketch you write, compile, upload, and watch over serial |
| [**BlinkEmbedded.md**](BlinkEmbedded.md) | Parts 2–6: the five files of a bare-metal Rust project (workspace manifests, `.cargo/config.toml`, `memory.x`, `build.rs`, `src/main.rs`), build, convert, flash, verify |
| [**Serial.md**](Serial.md) | Part 7: the board's three serial paths, SCI2 UART output from Rust, watched with a USB-UART adapter on D0/D1 |
| [**Matrix.md**](Matrix.md) | Part 8: charlieplexing from first principles, a HAL extension for the 11 matrix pins, empirical LED-mapping calibration, and a framebuffer that draws shapes |
| [**WiFi.md**](WiFi.md) | Part 9: the ESP32-S3's AT protocol spoken over SCI1 (P501/P502), credentials baked in from a `.env` at build time, and the join sequence that puts the firmware on your network |

Work in order: Basics → BlinkSketch → BlinkEmbedded → Serial → Matrix →
WiFi. Part/step numbering is continuous across the docs.

Reference — [**Limitations.md**](Limitations.md): plain-language glossary of
the pin/serial jargon, how the board's three serial routes work, and each
Rust-side shortcoming with its fix status.

Reference — [**HalSpec.md**](HalSpec.md): the spec for an independent (no-fork)
HAL crate that closes those gaps. **Implemented and published:** the spec's
crate is [`unor4-hal`](https://crates.io/crates/unor4-hal) 0.1.2
([docs](https://docs.rs/unor4-hal),
[source](https://github.com/TonyMarkham/unor4-hal)) — the HAL every doc in
this course now builds against.

## Status

- ✅ Part 0–1: board bring-up, C++ sketch round-trip, serial monitor
- ✅ Parts 2–6, first run: Rust workspace builds, firmware is **1,584 bytes**
  vs the 52 KB C++ baseline, upload + blink verified on hardware — but with
  the course's *original* HAL (`arduino-uno-r4-hal` 0.2). That run is history.
- ✅ HalSpec → `unor4-hal` 0.1.2 published (0.1.0/0.1.1 yanked). Its features
  were hardware-proven in the crate's own repo (see its README); the Minima
  pin map is build- and test-verified only — no Minima hardware exists.
- ⏳ Parts 2–6 and Part 7 as written now target `unor4-hal` — **the hardware
  checkpoints ahead are their first run against the new crate.** Until you
  pass them, treat clock init, GPIO, and delay as re-test territory.
- ⏳ Part 8 ([Matrix.md](Matrix.md)): doc written — needs the HAL's matrix
  extension (Step 8.1) and the hardware run to pass its checkpoints.
- ⏳ Part 9 ([WiFi.md](WiFi.md)): its `main.rs` joined a network on
  2026-10-21 as the standalone prototype `crates/wifi-sample`; the course
  crate path (Steps 9.1–9.7) has not been run as written.
- ⏳ Next: the roadmap below, toward the mission

## Roadmap

Each item is an exercise you build in `src/main.rs` (or a new crate) using
the HAL's docs (https://docs.rs/unor4-hal). Where the old HAL blocked a step,
`unor4-hal` now ships the missing piece — the list below is updated
accordingly:

1. **Serial output in Rust** — `uart::Uart::sci2` (SCI2) on D0/D1, watched
   with a USB-UART adapter ([Serial.md](Serial.md)). The polling driver has
   no `core::fmt::Write` impl by design — Part 7 has you write the six-line
   adapter yourself. No-adapter alternative: `Uart::sci9` on the internal
   monitor line P109/P110 (expected to appear in `arduino-cli monitor` via
   the bridge; not yet verified in this course).
2. **Button input** — D2 as pull-up input (`into_input_pullup`, `read`),
   edge detection.
3. **PWM breathing LED** — `pwm::Pwm` / `GptChannel`, `embedded-hal`
   `SetDutyCycle`; on the WiFi board D6 = P111 → GPT163/GTIOC3A.
4. **Analog reading** — `adc::Adc` on A0 (12- or 14-bit).
5. **Interrupts** — the `intc` module (ICU/IELSR event linking + dispatch)
   and the `uart-irq` feature: `UartIrq` ring-buffered serial with
   `embedded-hal-nb` / `embedded-io` / `core::fmt::Write` impls.
6. **USB CDC** — the `usb` feature: `usbfs` as a `usb-device` `UsbBus` plus
   the `CdcAcm` wrapper. Watch the mux: taking the USB port
   (`take_usb_port`, P408) disconnects the ESP32-S3 bridge — uploads stop
   until the port is released (SJ1 is the hardware escape).
7. **Data flash** — `dflash`: the 8 KB EEPROM-equivalent, byte-programmed,
   1 KB block erase — WiFi credential storage for provisioning.
8. **Protocol crate** — `crates/protocol`: the packet format, `no_std`
   compatible, tested on the host without hardware.
9. **LED matrix driver** — done as [Matrix.md](Matrix.md) (Part 8):
   charlieplexing, the HAL extension for the 11 matrix pins, empirical
   calibration of the LED-index→cell mapping, and a scanning framebuffer that
   draws shapes. Ownership settled from the datasheet: the matrix hangs off
   11 RA4M1 pins — the ESP32-S3 is not involved.
10. **WiFi** — done as [WiFi.md](WiFi.md) (Part 9): the bridge's AT
    protocol spoken over **SCI1 on P501/P502** (board-internal; header
    D0/D1 are not involved), with a plain `AT` handshake and a 10 µs
    receive poll — WiFi.md has the sources and the reasons.
    Credentials come from a `.env` file at build time. Beyond Part 9:
    sockets (`+BEGINCLIENT` / `+CLIENTSEND` / `+CLIENTRECEIVE`) for the
    mission's data packet, then decode it and render the pattern on the
    matrix. Mission complete.
11. **Bare-metal mode (optional)** — drop the HAL, drive registers via the
    `ra4m1` PAC directly. Background reading: Dom Wilson's series,
    https://domwil.co.uk/posts/uno-r4-rust/ (the basis of this course's
    original setup).

## Honest caveats

- **The current docs have not been hardware-run as written.** The verified
  1,584-byte blinky was built against `arduino-uno-r4-hal` 0.2; Parts 2–7
  have since been rewritten for `unor4-hal`, and those checkpoints are still
  ahead. Pass them before trusting any "what you should see" claim.
- `unor4-hal` 0.1.2's own features were hardware-proven during its
  development (its README records the witnessed runs), and docs.rs builds it
  with 100% documentation coverage. Its Minima pin map has no hardware
  behind it; this course only uses the WiFi map.
- The R4 WiFi's USB connector is shared: the ESP32-S3 bridge owns it by
  default — uploads, and C++ `Serial`, which on this board is P109/P110
  forwarded by the bridge. The mux (U2/U6, P408 control) hands it to the
  RA4M1's native USBFS only when firmware claims it: a C++ sketch that links
  the core's HID library, or the HAL's `usb` feature.
  Rust serial output that avoids the mux entirely is the SCI2/adapter path —
  [Serial.md](Serial.md) has the wiring and the sources (ABX00087 block
  diagram, ArduinoCore-renesas variant definitions, `uno-r4-wifi-usb-bridge`
  firmware).
- Rust on the R4's ESP32-S3 (the WiFi chip) is a separate, harder problem —
  the WiFi lesson depends on porting Arduino's bridge protocol; budget
  for it. The RA4M1 side of the link (SCI1) is no longer the hard part.
- Agent rules for this repo live in [AGENTS.md](AGENTS.md): the owner runs
  every step; agents write and maintain the docs only.
