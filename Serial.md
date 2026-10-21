# Serial — Part 7: your firmware's first words

Parts 2–6 proved the build-and-flash pipeline, but the only thing your firmware
could "say" was a blinking LED. This part adds the real debugging window: UART
output from Rust, printed once per second and watched from your PC.

> *This part has not yet been run on hardware in this course — it needs a
> USB-UART adapter. The "what you should see" descriptions are what the code
> produces by construction, not yet captured runs. Once you've run it, the
> outputs get replaced with real captures.*

Prerequisites:
- [BlinkEmbedded.md](BlinkEmbedded.md) — Parts 2–6 complete: the blinky builds,
  uploads, and blinks on hardware (now against `unor4-hal`).
- A **USB-UART adapter** (USB ↔ 2 or 3 wires). The wiring section explains the
  5 V requirement.
- Three jumper wires (female-female if your adapter has pin headers).

## The three serial paths on this board (read this first)

The UNO R4 WiFi has **three** serial paths. `unor4-hal` can reach all three —
but each one comes with a catch, which is why this lesson still uses an
adapter.

| # | Path | RA4M1 side | C++ name | Who uses it | Reachable from `unor4-hal`? |
|---|------|-----------|----------|-------------|------------------------------|
| 1 | USB-C ↔ analog switches ↔ RA4M1 native USBFS | RA4M1's USBFS pins | `SerialUSB` — live only in sketches that link the core's HID library | Nothing in this course so far | **Yes** — the `usb` feature (`CdcAcm`). Catch: claiming the port flips the USB mux away from the ESP32-S3 bridge — uploads stop until you release it. Not used in this lesson. |
| 2 | RA4M1 P109/P110 → level translator → ESP32-S3 UART0 → USB CDC | P109 (TX) / P110 (RX) | `Serial` | The ESP32 "USB bridge" firmware: sketch uploads (the bridge resets the RA4M1 into bootloader mode and forwards the SAM-BA transfer) and C++ `Serial` output — `arduino-cli monitor` in Part 1, where BlinkWipe's `tick` traveled | **Yes** — `Uart::sci9` on `p.sci9` / `p.sci9_tx` / `p.sci9_rx`. The same wire Part 1's `Serial` output took; from Rust, not yet verified in this course. |
| 3 | SCI2 → header pins D0/D1 | D1 = TX (P302), D0 = RX (P301) | `Serial1` | External serial devices and this lesson (the WiFi AT link is elsewhere — SCI1 on P501/P502, [WiFi.md](WiFi.md)) | **Yes** — `Uart::sci2`, this lesson |

Sources: the ABX00087 datasheet's block diagram (Arduino, rev. 18 Apr 2023),
the `ArduinoCore-renesas` 1.6.0 variant definitions (`UART1 = P109/P110`,
`UART2 = D1/D0`) and serial mapping (`boards.txt` builds this board with
`-DNO_USB`, under which `Arduino.h` makes `Serial` = `_UART1_` and
`Serial1` = `_UART2_`; native USB starts only when `initVariant`'s
`__maybe_start_usb` hook is overridden, which only `libraries/HID` does),
the `uno-r4-wifi-usb-bridge` firmware source, and the
`unor4-hal` source (its `uart` module docs carry the full verification chain
for the SCI9/SCI1 pin functions).

> [!IMPORTANT]
> This lesson deliberately uses **path 3 with a USB-UART adapter**, even
> though paths 1 and 2 are now reachable from Rust. Path 3 touches neither
> the USB mux nor the bridge's data line — nothing the course has proven so
> far (uploads, monitor) can break while you debug it. The board's own USB
> port will be silent for this firmware; that is expected, not a failure.

## Part 7 — Serial output from Rust

> **Step 7.1 — Wire the USB-UART adapter to D0/D1**
>
> Unplug the board's USB cable while wiring. Power stays on the board's own
> USB port afterwards; the adapter provides data and ground only.
>
> | USB-UART adapter | UNO R4 WiFi | Why |
> |---|---|---|
> | RX | **D1** | D1 is the board's TX — it cross-wires to the adapter's RX |
> | TX | **D0** | D0 is the board's RX — cross-wired the same way |
> | GND | GND | Two circuits need a common ground reference |
> | VCC / 5 V | — | **Leave unconnected** — the board powers itself over USB |
>
> ⚠ **Check the adapter's logic level.** The RA4M1 runs at 5 V and drives
> D0/D1 at 5 V. An adapter powered at 5 V, or whose inputs are specified
> 5 V-tolerant, is safe. A 3.3 V-only adapter needs a level shifter — check
> its datasheet, don't guess.
>
> This lesson only *transmits*, so the adapter's TX wire (to D0) can also stay
> unconnected if you prefer — but wire both now; the receive direction comes
> in a later part.

> **Step 7.2 — Find the adapter's COM port**
>
> With both USB cables connected (board + adapter):
>
> ```powershell
> arduino-cli board list
> ```
>
> You should now see two serial ports: the board's and the adapter's. If the
> adapter doesn't appear, its driver isn't installed — use Device Manager to
> find its COM number instead. The board's port (`$port`) is still the upload
> port; the adapter's is the one this lesson watches. Nothing to record yet —
> Step 7.6 captures the adapter's port in a variable.

**What/why:** two independent serial links now exist between PC and board: the
USB port (uploads, and the C++ `Serial` path) and the adapter (the UART your
Rust code drives). Keeping them separate in your head is the whole game on
this board.

> **Step 7.3 — Replace `crates\arduino-embedded-rust\src\main.rs`**
>
> ```rust
> //! Lesson 2: serial output — SCI2 UART on pins D0/D1, 115200 baud.
>
> #![no_std] // don't link the standard library — no OS down here
> #![no_main] // no standard main(); we provide our own entry point
>
> use core::fmt::{self, Write}; // brings write!/writeln! into scope for Printer
> use embedded_hal::delay::DelayNs;
> use panic_halt as _; // crash handler: halt the core on panic
> use unor4_hal::{Peripherals, uart::Uart};
>
> /// Adapter: the polling `Uart` speaks bytes, not formatting. This
> /// six-line wrapper gives it a `writeln!`-able surface by implementing
> /// `core::fmt::Write` on top of `write_bytes`.
> struct Printer(Uart);
>
> impl fmt::Write for Printer {
>     fn write_str(&mut self, s: &str) -> fmt::Result {
>         self.0.write_bytes(s.as_bytes());
>         Ok(())
>     }
> }
>
> // `#[entry]` becomes the function the vector table points to.
> // It can never return (`-> !`) — there is nothing to return *to*.
> #[cortex_m_rt::entry]
> fn main() -> ! {
>     // Claim all peripherals exactly once; also initializes the 48 MHz clock.
>     let p = Peripherals::take().unwrap();
>
>     let mut delay = p.delay; // SysTick-based blocking delay
>     let mut led = p.pins.d13.into_output(); // D13 = onboard LED
>
>     // SCI2 UART: TX = D1 (P302), RX = D0 (P301). Consumes the SCI2 handle
>     // and both pins. 115200 8N1.
>     let mut serial = Printer(Uart::sci2(p.sci2, p.pins.d1, p.pins.d0, 115_200, &p.clocks));
>
>     writeln!(serial, "serial alive").ok();
>
>     let mut n: u32 = 0;
>     loop {
>         writeln!(serial, "tick {}", n).ok();
>         n += 1;
>         led.toggle();
>         delay.delay_ms(1000);
>     }
> }
> ```

**What/why:**

- `Uart::sci2(p.sci2, p.pins.d1, p.pins.d0, 115_200, &p.clocks)` — five
  arguments, each a small lesson:
  - `p.sci2` — the SCI2 peripheral handle. Note the argument order:
    **sci, TX, RX** — TX first, and TX is **D1**. Swapping D0/D1 is the
    classic UART mistake; here the compiler catches it, because `d1` is a
    `Pin<'3', 2, …>` and the TX slot only accepts that type.
  - `p.pins.d1` / `p.pins.d0` — passed in their reset-default input mode. The
    constructor reconfigures both to the SCI peripheral function itself
    (type-state pins again: the `Uart` struct now *owns* them).
  - `115_200` — baud rate. 8N1 is the driver's fixed format. The constructor
    checks the baud against a 3 % error tolerance at the 48 MHz PCLKA (its
    baud calculator is unit-tested on the host); a value like 300 panics.
  - `&p.clocks` — the clock frequencies resolved during `Peripherals::take()`.
    The baud-rate divisor is computed from these — which is why clock init and
    UART setup can't be accidentally separated.
- `Uart::sci2` returns `Uart` directly, not a `Result` — an infeasible baud
  request panics into `panic-halt` and the LED stops (the crate's documented
  behavior for the polling constructors). Same failure strategy as
  `Peripherals::take().unwrap()`.
- `struct Printer(Uart)` — the polling `Uart` deliberately implements *no*
  formatting trait: it exposes `write_byte` / `write_bytes` /
  `read_byte` and nothing else. The `Printer` newtype is your first
  trait implementation on firmware: `write_str` hands the formatted bytes to
  `write_bytes` and returns `Ok(())` — the byte write blocks until each byte
  is out and cannot fail here, so there is nothing to report.
- `use core::fmt::{self, Write};` — `Write` is what makes `write!`/`writeln!`
  work on `Printer`; `fmt` is the `Result` type of `write_str`. Without the
  import, `writeln!` fails to compile with a confusing trait-not-in-scope
  error.
- `writeln!(serial, "tick {}", n).ok()` — formatting returns a `Result`. Our
  `write_str` always succeeds, so `.ok()` discards an always-`Ok` result. If
  you'd rather not think about it: `_ = writeln!(...)` is equivalent.
- The LED keeps blinking so the board shows "alive" even with the serial side
  of the desk unplugged.

✅ **Checkpoint:** `Get-Content crates\arduino-embedded-rust\src\main.rs` shows
the new imports (`unor4_hal`, `uart::Uart`) and the `Printer` trait impl.

---

> **Step 7.4 — Build**
>
> From the package directory (where `.cargo/config.toml` pins the target):
>
> ```powershell
> Set-Location crates\arduino-embedded-rust
> cargo build --release
> ```
>
> Expect a clean build. New text size versus the Part 4 blinky: the
> formatting machinery (`core::fmt`) isn't free. Optional, if you want the
> number: `cargo size --release`.

> **Step 7.5 — Convert and upload**
>
> Convert ELF → raw binary (same as Step 5.1):
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
>
> The adapter being wired to D0/D1 does not interfere with the upload — the
> upload travels the USB port and the bridge, path 2 in the table. Repeated
> mid-write `SAM-BA operation failed` → **DFU escape hatch** in Basics.md.

✅ **Checkpoint:** upload reports 100%.

---

> **Step 7.6 — Watch the output (on the adapter's port)**
>
> ```powershell
> $uartPort = 'COM7'   # <- your adapter's port from Step 7.2
> arduino-cli monitor -p $uartPort --config baudrate=115200
> ```
>
> What you should see: `serial alive` once, then `tick 0`, `tick 1`,
> `tick 2`, … one line per second, while the LED toggles in sync. Leave the
> monitor with Ctrl+C.
>
> If the monitor on the *board's* port (`$port`) is silent: correct — this
> firmware writes only to SCI2. Paths 1 and 2 from the table exist, but no
> line of this program touches them.

✅ **Checkpoint:** `tick N` lines streaming on the adapter's port, LED toggling
once per second. Part 7 complete — and you have a real debugging window for
every lesson that follows.

---

## Troubleshooting

- **Nothing at all, LED blinking** — TX/RX swapped (Step 7.1 table), adapter
  GND missing, or you're watching the board's port instead of the adapter's.
- **Garbage characters** — the monitor's baud doesn't match 115200, or the
  adapter's driver is set to a fixed baud. Re-run the monitor command from
  Step 7.6 exactly as written.
- **`writeln!` doesn't compile: "trait bound … not satisfied" or "trait is not
  in scope"** — the `use core::fmt::{self, Write};` line or the
  `impl fmt::Write for Printer` block is missing (Step 7.3).
- **Type mismatch on `Uart::sci2`** — TX/RX argument order is
  `(sci, tx, rx)`; TX is `d1`, RX is `d0` (Step 7.3).
- **Build succeeds, upload fails** — unrelated to this part: never-upload-cold
  (Step 7.5), then the DFU escape hatch in Basics.md.

Next: [Matrix.md](Matrix.md) — Part 8 lights up the board's 12×8 LED matrix:
charlieplexing, a HAL extension for the matrix's 11 pins, and your first
shapes. Its calibration step needs the serial link from this part.

Next after that: [WiFi.md](WiFi.md) — Part 9 puts the board on your network,
speaking the AT protocol with the same `Uart` driver on SCI1 (P501/P502,
board-internal). And after that: the
roadmap in [README](README.md) — toward the mission: receiving a data packet
over WiFi and displaying a pattern on the LED matrix.
