# Limitations — what we found, in plain language

No steps in this file. It's a reference: what the jargon means, how the
board's serial wiring actually works, and a plain-language list of every
shortcoming we've confirmed while building Part 7. Written after that
research; each fact lists where it came from.

## First, the words

| Term | What it actually is |
|---|---|
| **Header, D0–D13, A0–A5** | The holes along the board's edge. "D" pins are digital, "A" pins are analog. These are Arduino's numbers — printed on the board, used by every tutorial. |
| **P102, P301, P501, …** | The *chip's* own name for a physical pin ("Port 1, pin 02"). Same piece of metal, different name. Header hole **D13** is chip pin **P102**. Both naming systems appear in datasheets and source code, which is half the confusion. |
| **UART ("serial port")** | The simplest way two chips talk: two wires — **TX** (transmit) and **RX** (receive) — sending bytes one at a time at an agreed speed ("baud", here 115200). Cross-wiring TX↔RX is the classic mistake. |
| **SCI2, SCI9, SCI1** | The RA4M1 chip contains several *independent* UART engines, numbered SCI0–SCI9. Think of them as several mail rooms inside one building — each mail room can only serve certain windows (pins). SCI2 serves D0/D1, SCI9 serves P109/P110, SCI1 serves P501/P502. |
| **USB CDC** | A USB device that *pretends* to be a serial port. This is why a "COM" port appears when you plug the board in. |
| **The bridge** | The ESP32-S3's day job. It runs Arduino firmware ("UNOR4USBBridge") that presents the board to your PC as a USB device and shuttles bytes between the PC and the main chip. |
| **Level translator** | A chip between the two processors. The RA4M1 speaks 5 V, the ESP32-S3 speaks 3.3 V — connecting them directly would damage something. |
| **Mux (analog switch)** | Tiny switch chips (U2/U6) that decide who the USB plug's wires connect to: the ESP32-S3 (default) or the RA4M1's own USB hardware (only when firmware claims it — on this board, a C++ sketch that links the core's HID library, or `unor4-hal`'s `usb` feature). |
| **HAL** | "Hardware abstraction layer" — the Rust library (this course uses `unor4-hal`, the owner's own crate) that turns friendly Rust calls into register writes. |
| **PAC** | "Peripheral access crate" — auto-generated access to the chip's raw registers. Every feature, zero conveniences. Roadmap item 6 territory. |
| **Register** | A numbered mailbox inside the chip. Write the right value, the hardware acts. |
| **FSP** | Renesas's official C driver library — what Arduino's C++ core calls into underneath. |

## One pin, several names — the ones that matter here

| Where you'd touch it | Chip pin | UART engine | Where the wire goes |
|---|---|---|---|
| Header **D0** | P301 | SCI2 (RX) | header — external devices, Part 7's adapter |
| Header **D1** | P302 | SCI2 (TX) | header — external devices, Part 7's adapter |
| *(not on header)* | P109 | SCI9 (TX) | → level translator → ESP32-S3 → USB plug |
| *(not on header)* | P110 | SCI9 (RX) | ← ESP32-S3 ← USB plug |
| *(not on header)* | P501 | SCI1 (TX) | → ESP32-S3's WiFi AT server ([WiFi.md](WiFi.md)) |
| *(not on header)* | P502 | SCI1 (RX) | ← ESP32-S3's WiFi AT server |
| Header **D13** | P102 | — | onboard LED |

P109/P110 and P501/P502 are wired directly between the two chips — you can't
touch them from the header.

## The board's three serial "phone lines"

Two computers share one USB plug, and there are three separate routes a
serial byte can travel:

1. **USB plug ↔ mux switch ↔ RA4M1's own USB hardware.** Off by default on
   this board. The Arduino core flips the mux only for sketches that link its
   HID library (C++ `SerialUSB` works then); `unor4-hal`'s `usb` feature
   does the same from Rust.
2. **RA4M1 P109/P110 ↔ level translator ↔ ESP32-S3 ↔ USB plug.** The bridge's
   data line. Sketch *uploads* travel this route (the bridge resets the main
   chip into bootloader mode and forwards the transfer), and in C++ this is
   `Serial` — the path the Part 1 monitor used: BlinkWipe's `tick` lines
   traveled here.
3. **SCI2 ↔ header holes D0/D1.** Goes to the outside world — external
   serial devices, Part 7's adapter. In C++ this is `Serial1`.

A separate chip-to-chip line never reaches the USB plug or the header:
**SCI1 on P501/P502 ↔ the ESP32-S3's WiFi AT server** — the WiFi command
channel ([WiFi.md](WiFi.md)). The source that settles it: WiFiS3's modem is
constructed on `Serial2`, and with the `-DNO_USB` flag this board is built
with, the core maps `Serial2` to P501/P502.

Source: the ABX00087 datasheet's block diagram, plus the Arduino core and
bridge firmware source (links at the bottom).

## The shortcomings, one by one

Each finding below was confirmed during the course's first run, when the only
Rust HAL for this board was `arduino-uno-r4-hal` 0.2. They motivated
[HalSpec.md](HalSpec.md); its crate, **`unor4-hal` 0.1.2**, now exists — each
section carries its fix status.

### 1. The serial monitor can't hear SCI2 code — by wiring, not by bug

The original HAL's serial port was SCI2, which lands on header holes D0/D1 —
phone line 3. The monitor you've been using listens at the USB plug — phone
lines 1 and 2. The bytes physically have no path between them.

This is not a Rust failure. A C++ sketch writing to the same pins (`Serial1`)
is equally silent. The fix in Part 7 is a USB-UART adapter clipped onto D0/D1
— a $5-ish part that converts those header pins into its own USB port.

> **Status: physics unchanged, but the detours exist now.** `unor4-hal` also
> drives SCI9 (line 2 — the wire C++ `Serial` uses, so its output is
> expected in `arduino-cli monitor`; from Rust, unverified in this course)
> and native USB CDC (line 1 — via the `usb` feature, which takes the mux
> away from uploads).
> The adapter remains the path that touches nothing else, so Part 7 keeps it.

### 2. The old HAL implemented 1 of the chip's ~6 UART engines — and it was the least convenient one

The RA4M1 has several SCI engines. `arduino-uno-r4-hal` 0.2 implemented
exactly one: SCI2. The two it skipped were precisely the useful ones:

- **SCI9** (P109/P110) — phone line 2. A Rust UART here shows up in
  `arduino-cli monitor` with no adapter.
- **SCI1** (P501/P502) — the WiFi command channel. The mission depends on it
  (see #4).

> **Status: closed.** `unor4-hal` implements SCI2, SCI9, and SCI1 — polling
> 8N1 with a 3 % baud-error check, plus an interrupt-mode driver (`uart-irq`
> feature: `UartIrq` with ring buffers) for concurrent send/receive.

### 3. The old HAL had no USB support at all

The RA4M1 has USB hardware, and the C++ core proves it works (phone line 1 —
it drives it with **tinyusb**, a widely used open-source C USB stack). The
Rust side had generic USB stacks (`usb-device` + `usbd-serial`), but the
chip-specific driver piece that connects a stack to this chip's USB hardware
had, as far as I could find, never been published.

> **Status: closed.** The `usb` feature implements `usb-device`'s `UsbBus`
> for the RA4M1's USBFS, with a blocking CDC-ACM wrapper (`CdcAcm`) on top.
> The USB-mux takeover (P408) is an explicit, documented API step — and SJ1
> remains the hardware escape hatch.

### 4. Consequence for the mission: the WiFi lesson needed something the old library couldn't do

All WiFi commands go through the ESP32-S3 over SCI1 (P501/P502). Since the
old HAL only drove SCI2, the WiFi lesson (roadmap item 10) required either
extending the HAL or writing the UART at the register level.

> **Status: closed.** `unor4-hal` drives SCI1 (`Uart::sci1`), and Part 9
> ([WiFi.md](WiFi.md)) speaks the AT protocol over it. One catch with the
> polled driver: at 115200 the receive loop must poll faster than one byte
> time (~87 µs) or replies are lost to overrun — WiFi.md polls every 10 µs.

### 5. A documentation trap: the old HAL's published docs showed the *other* board's pin numbers

`arduino-uno-r4-hal` supported both UNO R4 boards behind feature flags
(`uno-r4-minima` / `uno-r4-wifi`), but its automatically generated docs were
built with the *Minima* feature — so the pin numbers looked wrong for this
board.

> **Status: closed by design.** In `unor4-hal`, the `wifi` map is the
> *default* (so plain docs.rs builds show this board), every pin's
> documentation shows **both** boards' mappings, and building with both maps
> — or neither — is a compile error. The WiFi map is additionally checked
> against a transcribed pin table by a host-side golden test.

## Escape hatches, ranked by effort

| Option | What it buys | Status |
|---|---|---|
| USB-UART adapter on D0/D1 (**Part 7 — works now**) | A serial window into Rust firmware | trivial (buy part, 3 wires) |
| ~~Extend the HAL with SCI9~~ | Monitor output with no adapter | **done** — `Uart::sci9` in `unor4-hal` (forwarding unverified in this course) |
| ~~Reach the WiFi AT channel from Rust~~ | Unlocks the WiFi lesson (mission-critical) | **done** — the channel is SCI1 on P501/P502; [WiFi.md](WiFi.md) speaks it |
| ~~Native USB CDC from Rust~~ | A USB serial port driven by the RA4M1 itself, no bridge in between | **done** — `usb` feature (`Usbfs` + `CdcAcm`); mux takeover documented |
| Full Renesas FSP via C bindings | Everything, the C++ way | days of integration; not worth it for this project |

## Where every fact above came from

- **ABX00087 product reference manual** (Arduino, rev. 18 Apr 2023) — block
  diagram showing P109/P110 ↔ ESP32 and P501/P502 ↔ ESP32, the mux/USB
  section, the two-chip overview.
- **ArduinoCore-renesas 1.6.0** (the C++ core) — `boards.txt` (`unor4wifi`
  is built with `-DNO_USB`); the `NO_USB` branch of `cores/arduino/Arduino.h`
  (`Serial` → `_UART1_`, `Serial1` → `_UART2_`, `Serial2` → `_UART3_`);
  `SerialObj1/2/3.cpp`, `variants/UNOWIFIR4/variant.cpp` and
  `pins_arduino.h` (`_UART1_` on P109/P110, `_UART2_` on D1/D0, `_UART3_` on
  P501 "TX WIFI" / P502 "RX WIFI"); `cores/arduino/main.cpp` (no
  `__USBStart()` under `NO_USB`) and `variant.cpp`'s `initVariant` →
  `__maybe_start_usb`, overridden only by `libraries/HID` (the mux flip);
  `variants/UNOWIFIR4/pinmux.inc` (which SCI engine serves which pin);
  `libraries/WiFiS3/src/Modem.cpp` (the modem on `Serial2`);
  `cores/arduino/Serial.cpp` (the core calls Renesas FSP C drivers
  underneath).
- **uno-r4-wifi-usb-bridge** (the ESP32-S3 firmware) — `UNOR4USBBridge.ino`:
  the bridge forwards its UART to the USB CDC port unconditionally, and
  toggles the main chip's reset/boot lines for uploads.
- **arduino-uno-r4-hal v0.2.0** (the course's original Rust library) — source
  on docs.rs: `serial.rs` (SCI2-only, 8N1, the baud-rate math), `gpio.rs` (the
  feature-gated pin tables compared against the core's tables).
- **unor4-hal v0.1.2** (the course's current HAL, the HalSpec implementation) —
  crates.io / docs.rs / github.com/TonyMarkham/unor4-hal: the `uart`, `usb`,
  `intc`, and `gpio` module docs carry the pin-function and register
  verification chains cited above.
