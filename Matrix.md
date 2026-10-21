# Matrix — Part 8: the 12×8 LED matrix, driven by hand

The board's most distinctive feature: 96 red LEDs, no driver chip, no shift
registers — 11 bare pins and your code. This part builds a matrix driver from
first principles and draws shapes on it.

> *This part has not yet been run on hardware in this course. The code is
> built entirely from verified facts (listed per step, with sources), but
> "what you should see" descriptions become real only at your checkpoints.
> Once you've run it, the outputs get replaced with captures.*

Prerequisites:
- [BlinkEmbedded.md](BlinkEmbedded.md) — Parts 2–6 complete on `unor4-hal`.
- [Serial.md](Serial.md) — Part 7 complete: the USB-UART adapter on D0/D1 is
  wired, because Step 8.3 uses serial output to calibrate the matrix.
- The `unor4-hal` source clone (Step 8.1 extends it).

## How this display actually works (read this first)

The matrix has no controller. It is **charlieplexed**: 96 LEDs wired across
11 pins, where every LED sits between a *different pair* of pins. An LED has
one direction, so:

- Set pin A **high** and pin B **low** → the LED between A and B lights.
- Make a pin a plain **input** (Hi-Z, third state) → it's electrically
  invisible; every LED touching it is off.
- You can therefore address 11 × 10 = 110 LEDs with 11 pins. This board
  uses 96 of the 110 possible pairs.

Lighting one LED at a time, fast enough that your eye can't tell, *is* the
display. A "frame" is just the list of which LEDs are on during each scan
cycle.

Verified facts this part builds on (sources at the bottom):

- **Who owns the matrix: the RA4M1, not the ESP32-S3.** The ABX00087
  datasheet lists the matrix's pins explicitly as RA4M1 pins: P003, P004,
  P011, P012, P013, P015, P204, P205, P206, P212, P213 — board-internal,
  none of them header pins.
- **Which pair forms which LED.** The `Arduino_LED_Matrix` library's
  `pins[]` table (96 `(anode, cathode)` entries) is the pair map. We
  transcribe it as data — the pairs come from the board schematic, the
  enumeration order from the library.
- **How the C++ library drives it:** `turnLed()` releases *all 11* pins to
  Hi-Z, then drives the one LED's anode high and cathode low. A timer ISR
  cycles through all 96 slots at 10 kHz — every LED gets a 1/96 duty slot
  ~104 times per second. Our driver does the same thing from the main loop.
- **The gap in `unor4-hal` 0.1.2:** its `gpio::Pins` exposes only the 20
  header pins. The 11 matrix pins are not reachable yet — Step 8.1 fixes
  that in the HAL crate (your code, per this repo's rules: you type it).

## Part 8 — The matrix driver

> **Step 8.1a — Write `src/gpio/matrix.rs` in the `unor4-hal` repo**
>
> In your `unor4-hal` clone, create `src/gpio/matrix.rs`:
>
> ```rust
> //! The 12×8 LED matrix (feature `wifi`): 96 red LEDs charlieplexed
> //! across 11 board-internal pins. One LED lit at a time; the caller
> //! scans.
> //!
> //! Facts: pin list from the ABX00087 datasheet; index→pair table from
> //! `Arduino_LED_Matrix`'s `pins[]` (behavioral reference — the pairs
> //! are schematic facts); PFS/PWPR handling identical to
> //! `src/gpio/pin.rs`.
>
> /// Base of the `PmnPFS` per-pin register array (RA4M1 manual, I/O
> /// Ports chapter) — same constant as `src/gpio/pin.rs`.
> const PFS_BASE: usize = 0x4004_0800;
> /// `PWPR` (inside PMISC) — the PFS write-protect gate.
> const PWPR: *mut u8 = 0x4004_0d03 as *mut u8;
> /// `PDR` — port direction bit (0 = input/Hi-Z, 1 = output).
> const PDR: u32 = 1 << 2;
>
> /// The 11 matrix pins, indexed by the Arduino core's matrix pin
> /// offsets 0–10 (`variants/UNOWIFIR4/variant.cpp`, pins 28–38, which
> /// are the datasheet's matrix pin list in order).
> const MATRIX_PINS: [(char, u8); 11] = [
>     ('0', 3),  // offset 0 — P003
>     ('0', 4),  // offset 1 — P004
>     ('0', 11), // offset 2 — P011
>     ('0', 12), // offset 3 — P012
>     ('0', 13), // offset 4 — P013
>     ('0', 15), // offset 5 — P015
>     ('2', 4),  // offset 6 — P204
>     ('2', 5),  // offset 7 — P205
>     ('2', 6),  // offset 8 — P206
>     ('2', 12), // offset 9 — P212
>     ('2', 13), // offset 10 — P213
> ];
>
> /// LED index → `(anode offset, cathode offset)`. Transcribed from
> /// `Arduino_LED_Matrix`'s `pins[]` table (96 entries, first element is
> /// the pin driven HIGH).
> const LED_PAIRS: [(u8, u8); 96] = [
>     (7, 3), (3, 7), (7, 4), (4, 7), (3, 4), (4, 3),
>     (7, 8), (8, 7), (3, 8), (8, 3), (4, 8), (8, 4),
>     (7, 0), (0, 7), (3, 0), (0, 3), (4, 0), (0, 4),
>     (8, 0), (0, 8), (7, 6), (6, 7), (3, 6), (6, 3),
>     (4, 6), (6, 4), (8, 6), (6, 8), (0, 6), (6, 0),
>     (7, 5), (5, 7), (3, 5), (5, 3), (4, 5), (5, 4),
>     (8, 5), (5, 8), (0, 5), (5, 0), (6, 5), (5, 6),
>     (7, 1), (1, 7), (3, 1), (1, 3), (4, 1), (1, 4),
>     (8, 1), (1, 8), (0, 1), (1, 0), (6, 1), (1, 6),
>     (5, 1), (1, 5), (7, 2), (2, 7), (3, 2), (2, 3),
>     (4, 2), (2, 4), (8, 2), (2, 8), (0, 2), (2, 0),
>     (6, 2), (2, 6), (5, 2), (2, 5), (1, 2), (2, 1),
>     (7, 10), (10, 7), (3, 10), (10, 3), (4, 10), (10, 4),
>     (8, 10), (10, 8), (0, 10), (10, 0), (6, 10), (10, 6),
>     (5, 10), (10, 5), (1, 10), (10, 1), (2, 10), (10, 2),
>     (7, 9), (9, 7), (3, 9), (9, 3), (4, 9), (9, 4),
> ];
>
> #[inline]
> fn pfs_addr(port: char, pin: u8) -> *mut u32 {
>     let p = (port as u8 - b'0') as usize;
>     (PFS_BASE + 0x40 * p + 4 * pin as usize) as *mut u32
> }
>
> #[inline]
> fn port_block(port: char) -> &'static ra4m1::port0::RegisterBlock {
>     let p = (port as u8 - b'0') as usize;
>     // All port register blocks share one layout, spaced 0x20 apart.
>     unsafe { &*((0x4004_0000 + 0x20 * p) as *const ra4m1::port0::RegisterBlock) }
> }
>
> /// Unlocks `PWPR`, read-modify-writes one `PmnPFS`, relocks — the same
> /// critical-section-guarded sequence as `src/gpio/pin.rs`.
> fn modify_pfs(port: char, pin: u8, clear: u32, set: u32) {
>     critical_section::with(|_| unsafe {
>         core::ptr::write_volatile(PWPR, 0x00); // B0WI = 0
>         core::ptr::write_volatile(PWPR, 0x40); // PFSWE = 1
>         let addr = pfs_addr(port, pin);
>         let v = core::ptr::read_volatile(addr);
>         core::ptr::write_volatile(addr, (v & !clear) | set);
>         core::ptr::write_volatile(PWPR, 0x00); // PFSWE = 0
>         core::ptr::write_volatile(PWPR, 0x80); // B0WI = 1
>     });
> }
>
> /// Sets a pin's direction bit (after latching the output level).
> fn set_direction(port: char, pin: u8, output: bool) {
>     if output {
>         modify_pfs(port, pin, 0, PDR);
>     } else {
>         modify_pfs(port, pin, PDR, 0);
>     }
> }
>
> /// Handle to the matrix's 11 pins. All pins are Hi-Z inputs whenever
> /// no LED is being driven.
> pub struct MatrixPins {
>     _port2: ra4m1::PORT2,
> }
>
> impl MatrixPins {
>     /// Claims PORT2 (five of the matrix pins live there). The other
>     /// six live on PORT0, which `Pins` has already claimed as an
>     /// ownership token and never drives; this module addresses them by
>     /// fixed address, like `pin.rs` does for every pin.
>     pub fn new(port2: ra4m1::PORT2) -> Self {
>         Self { _port2: port2 }
>     }
>
>     /// Releases all 11 pins to Hi-Z inputs — the dark state. Also the
>     /// first half of every [`light`](Self::light) call.
>     pub fn release(&mut self) {
>         for &(port, pin) in &MATRIX_PINS {
>             set_direction(port, pin, false);
>         }
>     }
>
>     /// Drives LED `led` (0–95): anode high, cathode low, everything
>     /// else Hi-Z. Indexes past 95 leave the display dark — the scan
>     /// loop never has to bounds-check.
>     pub fn light(&mut self, led: usize) {
>         self.release();
>         if let Some(&(a, c)) = LED_PAIRS.get(led) {
>             let (aport, apin) = MATRIX_PINS[usize::from(a)];
>             let (cport, cpin) = MATRIX_PINS[usize::from(c)];
>             // Latch the levels first, then enable the drivers — no
>             // glitch escapes mid-configuration (same ordering as
>             // `pin.rs::into_output_in_state`).
>             port_block(aport).posr().write(|w| unsafe { w.bits(1u16 << apin) });
>             port_block(cport).porr().write(|w| unsafe { w.bits(1u16 << cpin) });
>             set_direction(aport, apin, true);
>             set_direction(cport, cpin, true);
>         }
>     }
> }
> ```

> **Step 8.1b — Register the module and claim the peripheral**
>
> Three small edits in the same repo. In `src/gpio/mod.rs`, next to the
> other `pub mod`/`pub use` lines, add:
>
> ```rust
> pub mod matrix;
> pub use matrix::MatrixPins;
> ```
>
> In `src/peripherals.rs`, add the field (with the other WiFi-only
> fields, `sci9_tx`/`sci9_rx`):
>
> ```rust
> /// The 12×8 LED matrix's 11 charlieplex pins (WiFi board only; the
> /// matrix is board-internal — ABX00087 datasheet).
> #[cfg(feature = "wifi")]
> pub matrix: gpio::MatrixPins,
> ```
>
> and in `Peripherals::take()`, add the construction (PORT2 is the one
> port `Pins::new` doesn't yet claim):
>
> ```rust
> matrix: gpio::MatrixPins::new(device.PORT2),
> ```

> **Step 8.1c — Build and test the HAL**
>
> From the `unor4-hal` repo root:
>
> ```powershell
> cargo build
> ```
>
> ```powershell
> cargo test
> ```
>
> Expected: both succeed, including the existing pin-table golden test.
> If `cargo build` complains about `device.PORT2`, the `Peripherals`
> struct edit and the `take()` edit are out of sync — re-check Step 8.1b.

✅ **Checkpoint:** the HAL builds and its host tests pass with `matrix.rs`
in place. From here on, everything runs in the firmware crate.

---

> **Step 8.2 — Point the firmware at your HAL build**
>
> The extension isn't published, so the firmware uses it by path. In the
> **root** `Cargo.toml` of the firmware workspace, change the `unor4-hal`
> line (adjust the path to wherever your `unor4-hal` clone lives; this
> assumes it sits next to this repo under `D:\git\`):
>
> ```toml
> unor4-hal          = { path = "../../../unor4-hal" }
> ```
>
> A path dependency compiles your local source, so HAL edits show up in
> the firmware with no publishing step. When you later publish 0.1.3
> with the matrix support, switch back to the version dependency.

> **Step 8.3 — Calibrate: which LED index is which cell?**
>
> The datasheet shows the LED-number→grid mapping only as an image; this
> course derives it empirically. This firmware lights LED 0, then 1, …
> then 95 — a slow chase — while printing each index over the Part 7
> serial link. Watch the matrix and write down which cell each index
> lights. Replace `crates\arduino-embedded-rust\src\main.rs`:
>
> ```rust
> //! Lesson 3: matrix calibration — the LED chase, index → cell.
>
> #![no_std] // don't link the standard library — no OS down here
> #![no_main] // no standard main(); we provide our own entry point
>
> use core::fmt::{self, Write};
> use embedded_hal::delay::DelayNs;
> use panic_halt as _;
> use unor4_hal::{Peripherals, gpio::MatrixPins, uart::Uart};
>
> /// Adapter: the polling `Uart` speaks bytes, not formatting (Part 7).
> struct Printer(Uart);
>
> impl fmt::Write for Printer {
>     fn write_str(&mut self, s: &str) -> fmt::Result {
>         self.0.write_bytes(s.as_bytes());
>         Ok(())
>     }
> }
>
> #[cortex_m_rt::entry]
> fn main() -> ! {
>     let p = Peripherals::take().unwrap();
>
>     let mut delay = p.delay;
>     let mut matrix = p.matrix;
>     let mut serial = Printer(Uart::sci2(p.sci2, p.pins.d1, p.pins.d0, 115_200, &p.clocks));
>
>     let mut led: usize = 0;
>     loop {
>         writeln!(serial, "led {}", led).ok();
>         matrix.light(led);
>         delay.delay_ms(700);
>         matrix.release();
>         delay.delay_ms(300);
>         led = (led + 1) % 96;
>     }
> }
> ```
>
> A single lit LED needs no scanning — one pin pair stays driven, so the
> chase is steady, not flickery. `release()` between steps makes each
> cell blink on/off distinctly, so you can tell index N apart from N+1
> when the chase wraps a corner.

Then build, convert, and upload it — same pipeline as every part:

> **Step 8.3a — Build**
>
> From the package directory (where `.cargo/config.toml` pins the target):
>
> ```powershell
> Set-Location crates\arduino-embedded-rust
> cargo build --release
> ```

> **Step 8.3b — Convert ELF → raw binary**
>
> ```powershell
> cargo objcopy --release -- -O binary firmware.bin
> ```

> **Step 8.3c — Upload**
>
> ⚠ **Never upload cold** — double-tap reset (LED fades = bootloader mode),
> re-check the port with `arduino-cli board list`, re-set `$port` if it
> changed, then (from the package directory, where `firmware.bin` lives):
>
> ```powershell
> arduino-cli upload -p $port --fqbn arduino:renesas_uno:unor4wifi --input-file firmware.bin
> ```

> **Step 8.3d — Watch and record the mapping**
>
> Open the monitor on the adapter's port (Part 7, Step 7.6):
>
> ```powershell
> $uartPort = 'COM7'   # <- your adapter's port
> arduino-cli monitor -p $uartPort --config baudrate=115200
> ```
>
> Each line names the index currently lit: `led 0`, `led 1`, … As each
> prints, note **which cell of the 12×8 grid** is lit (columns 0–11 left
> to right, rows 0–7 top to bottom). After the chase finishes — 96 × 1
> second — you have the full mapping. 96 one-second slots is a long
> minute; you can watch passively and rewind the monitor output, or
> change the delays and re-upload.

✅ **Checkpoint:** a filled-in table mapping every LED index 0–95 to its
(row, column) cell. The next step turns that table into code.

---

> **Step 8.4 — The real driver: frames, scanning, shapes**
>
> Now the display becomes a framebuffer. This firmware defines a
> `Frame` (8 rows × 12 columns of `bool`), converts it to LED slots
> through **your** calibration table, and scans it out at the same rate
> the C++ library uses (one 96-slot cycle every ~9.6 ms ≈ 104 Hz — each
> lit LED gets a 1/96 duty slot, identical to the C++ library's timer
> ISR). Replace `crates\arduino-embedded-rust\src\main.rs`:
>
> ```rust
> //! Lesson 4: the 12×8 LED matrix — charlieplexed shapes.
>
> #![no_std] // don't link the standard library — no OS down here
> #![no_main] // no standard main(); we provide our own entry point
>
> use embedded_hal::delay::DelayNs;
> use panic_halt as _;
> use unor4_hal::{Peripherals, gpio::MatrixPins};
>
> /// 8 rows × 12 columns, row-major. `true` = LED on.
> type Frame = [[bool; 12]; 8];
>
> /// LED index at each (row, column) — **measured** in Step 8.3, not
> /// assumed. Fill this in from your calibration notes before building.
> const LED_AT: [[u8; 12]; 8] = [
>     [0; 12], [0; 12], [0; 12], [0; 12],
>     [0; 12], [0; 12], [0; 12], [0; 12], // <- replace every row
> ];
>
> /// Duration of one LED slot. 96 slots × 100 µs ≈ 9.6 ms per frame —
> /// the C++ library's scan rate (10 kHz per LED, 1/96 duty each).
> const SCAN_US: u32 = 100;
>
> /// Scans one full frame: for each grid cell, drive its LED or leave
> /// the pins Hi-Z, always one slot long, so brightness is uniform.
> fn show(matrix: &mut MatrixPins, delay: &mut impl DelayNs, frame: &Frame) {
>     for row in 0..8 {
>         for col in 0..12 {
>             if frame[row][col] {
>                 matrix.light(usize::from(LED_AT[row][col]));
>             } else {
>                 matrix.release();
>             }
>             delay.delay_us(SCAN_US);
>         }
>     }
> }
>
> /// A heart, 12 wide × 8 tall.
> const HEART: Frame = [
>     [false, false, true, true, false, false, false, false, true, true, false, false],
>     [false, true, true, true, true, false, false, true, true, true, true, false],
>     [false, true, true, true, true, true, true, true, true, true, true, false],
>     [true, true, true, true, true, true, true, true, true, true, true, true],
>     [true, true, true, true, true, true, true, true, true, true, true, true],
>     [false, true, true, true, true, true, true, true, true, true, true, false],
>     [false, false, true, true, true, true, true, true, true, true, false, false],
>     [false, false, false, true, true, true, true, true, true, false, false, false],
> ];
>
> #[cortex_m_rt::entry]
> fn main() -> ! {
>     let p = Peripherals::take().unwrap();
>
>     let mut delay = p.delay;
>     let mut matrix = p.matrix;
>
>     loop {
>         show(&mut matrix, &mut delay, &HEART);
>     }
> }
> ```
>
> **What/why:**
> - `LED_AT` is the calibration table made const. Every entry is your own
>   measurement — this is the one thing in the whole course you cannot
>   copy from a document, because the only published form of this mapping
>   is a picture in the datasheet.
> - `show()` never leaves a LED lit longer than one slot, and lights at
>   most one LED at any instant — the charlieplexing contract. Breaking
>   that (driving two LEDs "at once") is what ghost images are made of.
> - The scan runs from the main loop, not a timer ISR. A timer-plus-IRQ
>   scan (the C++ library's way) is the natural follow-up exercise — it
>   frees the main loop, and `unor4-hal`'s `intc`/GPT are exactly the
>   machinery for it.
> - Swap `HEART` for your own shapes: a border (`row == 0 || row == 7 ||
>   col == 0 || col == 11`), a checkerboard (`(row + col) % 2 == 0`),
>   or an animation (a `Frame` sequence and a counter in the loop).

> **Step 8.5 — Build, convert, upload**
>
> ```powershell
> cargo build --release
> ```
>
> ```powershell
> cargo objcopy --release -- -O binary firmware.bin
> ```
>
> ⚠ **Never upload cold** — double-tap reset (LED fades = bootloader mode),
> re-check the port with `arduino-cli board list`, re-set `$port` if it
> changed, then (from the package directory, where `firmware.bin` lives):
>
> ```powershell
> arduino-cli upload -p $port --fqbn arduino:renesas_uno:unor4wifi --input-file firmware.bin
> ```

✅ **Checkpoint:** a steady heart on the matrix. If it renders but looks
wrong, Troubleshooting below; if it renders right, Part 8 is done — the
board's display now answers to your own driver, and the mission's "render
the pattern" half has its output device.

---

## Troubleshooting

- **Build error: `no field matrix` on `Peripherals`** — Step 8.1b's field
  addition is missing, or the firmware is still resolving the published
  `unor4-hal` instead of your path dependency (Step 8.2). `cargo update -p
  unor4-hal` forces a re-resolve after switching to the path dependency.
- **Whole matrix dark, build and upload clean** — every pin stuck in one
  direction: check that `release()` runs and that `LED_AT` doesn't point
  every cell at index 0 (the placeholder table shows as a single dim cell).
- **One wrong cell** — a swapped entry in `LED_AT` (Step 8.4), not a driver
  bug: re-check your calibration notes for that index.
- **Ghost images / faint extra LEDs** — a pin that should be Hi-Z is
  driving: the `release()` call must run before every new `light()`, and
  the cathode must be a true output-low, not an input.
- **Uniform flicker** — the scan is too slow. `SCAN_US` of 100 keeps the
  frame at ~104 Hz; long `delay_ms` calls anywhere in the loop will show
  as visible flicker.
- **Build succeeds, upload fails** — unrelated to this part: never-upload-cold
  (Step 8.3c / 8.5), then the DFU escape hatch in Basics.md.

## Where every fact above came from

- **ABX00087 datasheet** (Arduino) — the matrix is charlieplexed across
  11 named RA4M1 pins (P003…P213); the LED-number→grid mapping appears
  only as an image (hence Step 8.3's empirical calibration).
- **ArduinoCore-renesas, `variants/UNOWIFIR4/variant.cpp`** — `g_pin_cfg`
  entries 28–38 map Arduino pins D28–D38 to the 11 matrix pins, in the
  order the `pins[]` table's offsets use.
- **ArduinoCore-renesas, `libraries/Arduino_LED_Matrix/src/Arduino_LED_Matrix.h`**
  — the 96-entry `pins[]` (anode, cathode) table; `turnLed()`'s
  release-then-drive discipline; the 10 kHz timer scan, one LED per tick
  against a 96-bit framebuffer.
- **`unor4-hal` `src/gpio/pin.rs`** — the PFS base address, PWPR
  unlock/lock sequence, PDR bit, and latch-before-direction ordering the
  HAL extension reuses.
