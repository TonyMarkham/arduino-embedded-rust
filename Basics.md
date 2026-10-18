# Basics — hardware, reset button, toolchain, troubleshooting

This is the reference doc for the course. Read it once top-to-bottom, then keep
it open. The hands-on parts live in [BlinkSketch.md](BlinkSketch.md) (Part 1)
and [BlinkEmbedded.md](BlinkEmbedded.md) (Parts 2–6); the
[README](README.md) is the summary.

**How to use the docs:** *You* type every command and write every file yourself.
Each step is inside a blockquote (`>`) — that's the thing you do, from its top
to its bottom. The text between blockquotes explains the concept behind what
you just did. Don't copy-paste blindly — typing it and breaking it is where
the learning happens.

**On version numbers:** the dependency versions in these docs are deliberate
pins (see `Cargo.toml`), verified when written. Commands that talk to live
services (`winget`, `arduino-cli`) print their own current truth — trust their
output over any number here.

---

## The hardware in 60 seconds

The UNO R4 WiFi has **two** chips:

| Chip | Role | What you'll target |
|---|---|---|
| **Renesas RA4M1** (R7FA4M1AB3CFM) | The main MCU: ARM Cortex-M4F @ 48 MHz, 256 KB flash, 32 KB RAM, runs at 5 V | ✅ All your Rust code |
| **ESP32-S3** | WiFi/BT co-processor. Also owns the USB-C connector: it runs the USB-to-UART bridge firmware your PC talks to | ❌ (runs Arduino's firmware, leave it alone) |

Three facts that shape everything below:

1. The board ships with a **bootloader in the first 16 KiB of flash**. It lets
   you flash over plain USB. Your programs must therefore live *after* it
   (flash address `0x4000`), or you erase the bootloader and lose the ability
   to flash over USB (recovery then requires a hardware debug probe — avoid).
2. Flashing happens two ways: **bossac** writes over the USB serial link
   (through the ESP32-S3 bridge — occasionally flaky mid-write, see
   Troubleshooting below), or **DFU** writes over raw USB in bootloader mode
   (the reliable escape hatch).
3. The MCU is a **Cortex-M4F**, so the Rust compile target is
   `thumbv7em-none-eabihf` (`-hf` = hardware float).

---

## The reset button — one button, two behaviors

> [!IMPORTANT]
> Read this once before Part 1. Most upload confusion on this board comes
> from not knowing which behavior you're invoking.
>
> | You do | The board does | How you can tell it worked |
> |---|---|---|
> | **Single press** | The MCU reboots. The bootloader runs briefly, finds no upload request, and starts the installed sketch. | The sketch restarts (its LED pattern starts over). |
> | **Double-tap** (two quick presses) | The bootloader **stays in bootloader mode**: it waits for firmware over USB and does *not* run the sketch. | The board LED fades in and out, continuously. |
>
> **What each behavior is used for in this course:**
>
> 1. **Reliable uploads** — a "cold" upload (board running its sketch) triggers a
>    reset handshake that re-enumerates the USB port *while* the flasher is
>    already talking — a race that kills uploads mid-write (`SAM-BA operation
>    failed`). Uploading into a board already waiting in bootloader mode
>    (double-tap) avoids the race entirely. That's why every upload step in the
>    course says **"Never upload cold."**
> 2. **Finding an invisible board** — if the board doesn't appear in
>    `arduino-cli board list`, double-tap forces it into bootloader mode where it
>    re-enumerates as a fresh USB device.
> 3. **The DFU escape hatch** (Troubleshooting, below) — double-tap is also what
>    puts the board into DFU mode for the raw-USB flasher. When flashing
>    completes, a **single press** exits bootloader mode and boots the new
>    sketch.
>
> **Two gotchas:**
>
> - The COM port number **can change** whenever the board enters or leaves
>   bootloader mode. After any reset action, re-run `arduino-cli board list`
>   before uploading, and re-set `$port` if it changed.
> - After a double-tap, the board intentionally runs *nothing* — a dark LED
>   matrix is normal in bootloader mode. The fade is the tell that it's alive
>   and waiting.
>
> **How to check which mode the board is in:**
>
> ```powershell
> Get-PnpDevice -PresentOnly | Where-Object InstanceId -match 'VID_2341'
> ```
>
> - `PID_006D` present = sketch mode (the running sketch owns USB).
> - `PID_1002` present = bootloader mode.
>
> ⚠ **Unverified on hardware:** the `PID_1002` = bootloader mapping comes
> from the core's `boards.txt` (`upload.pid`), not yet proven on this board —
> the DFU escape hatch relies on the same ID.

---

## Part 0 — Toolchain

> **Step 0.1 — Check the Rust side**
>
> Open PowerShell 7 in this repo folder (`D:\git\arduino-embedded-rust`) and run:
>
> ```powershell
> rustup target list --installed     # must list thumbv7em-none-eabihf
> cargo objcopy --version            # cargo-binutils, used in Part 5
> ```
>
> If both succeed, skip Step 0.2.

**What/why:** the `thumbv7em-none-eabihf` target is what produces bare-metal ARM
machine code. `cargo-binutils` provides `cargo objcopy` etc. without a full GNU
ARM toolchain install.

> **Step 0.2 — Install anything missing from Step 0.1**
>
> ```powershell
> rustup target add thumbv7em-none-eabihf   # the compiler's ARM Cortex-M4F backend
> cargo install cargo-binutils              # rust-flavored objdump/objcopy/size
> rustup component add llvm-tools           # the binaries cargo-binutils wraps (size, objcopy)
> ```

> **Step 0.3 — Install arduino-cli**
>
> ```powershell
> winget install -e --id ArduinoSA.CLI
> ```
>
> Open a **new terminal** afterwards so `arduino-cli` lands on your PATH
> (verify with `Get-Command arduino-cli`).

> **Step 0.4 — Install the board package for the UNO R4**
>
> This also bundles the flasher tools (`bossac`, `dfu-util`):
>
> ```powershell
> arduino-cli core install arduino:renesas_uno
> ```
>
> If "not found", list what your index offers and use the renesas name it
> shows:
>
> ```powershell
> arduino-cli core search renesas
> ```

✅ **Checkpoint:** the Step 0.1 checks pass and `arduino-cli core list` shows
`arduino:renesas_uno`.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| Board powers (green LED) but never appears in `board list` | Confirmed cause in this course: charge-only USB cable. Swap to a known data cable; try another USB port; avoid hubs. |
| `SAM-BA operation failed` mid-write | Known intermittent R4 issue (ArduinoCore-renesas #10/#73). Retry the same command; after 3+ consecutive failures use the **DFU escape hatch** below. An erase-only state is harmless. Observed in this course: uploads failed 9+ times while the board was powered by *both* barrel jack and USB-C; the first USB-only power cycle uploaded cleanly. Correlation, not proven cause — but power the board from USB alone while flashing. |
| Upload says permission denied / port busy | Close any serial monitor (including the Arduino IDE). |
| Upload dies, then `No device found on COM3` | The 1200-bps touch may have changed the port number — re-run `arduino-cli board list` and re-set `$port`. |
| Board doesn't blink after a 100% upload | Power-cycle once (unplug/replug the USB-C cable — it is both power and data on this board), watch the small amber LED by pin 13 — not the LED matrix, which Rust sketches never drive. Then run the serial check (Step 1.4d pattern in BlinkSketch.md). |
| LED blinks too fast/slow | Clock configuration issue — the HAL's `Peripherals::take()` sets 48 MHz; make sure you call it. |
| **Bootloader overwritten** (no uploads possible at all) | Recovery requires reflashing the bootloader via a hardware debug probe or the Renesas boot-pin procedure — see `bootloaders/UNO_R4` in the ArduinoCore-renesas repo. This is why memory.x starts at 0x4000. |

### DFU escape hatch (when SAM-BA keeps failing)

bossac writes firmware over the serial link *through the ESP32-S3 bridge* —
that hop is what fails mid-write. DFU writes over raw USB and skips it. The
board's bootloader speaks DFU natively (device ID `2341:1002`, from the core's
boards.txt):

1. Double-tap reset. The board LED fades in and out = DFU bootloader mode
   (per Arduino's support docs).
2. Confirm the DFU device appeared:
   ```powershell
   Get-PnpDevice -PresentOnly | Where-Object InstanceId -match 'VID_2341'
   ```
   Expect an entry with `PID_1002`.
3. Locate the bundled dfu-util:
   ```powershell
   Get-ChildItem -Recurse -Filter dfu-util.exe $env:LOCALAPPDATA\Arduino15\packages\arduino\tools |
     Select-Object -First 1 -ExpandProperty FullName
   ```
4. Flash (substitute the path from step 3):
   ```powershell
   & "<dfu-util path>" --device 2341:1002 -D build\BlinkWipe.ino.bin -a0 -Q
   ```
   For a Rust firmware (BlinkEmbedded.md Part 5): `-D firmware.bin` instead.
5. Single-press reset — exits bootloader mode and boots the new sketch.

⚠ **Unverified plumbing:** the `-a0` interface number and the DFU-mode device
ID `2341:1002` are inferred from the core's boards.txt, not yet proven on
hardware. If dfu-util reports "No DFU capable USB device", report the output
of step 2 — that decides between driver and mode issues.
