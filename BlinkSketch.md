# BlinkSketch — Part 1: prove the flash pipeline with a C++ sketch

This is Part 1 of the course. The goal: prove the full USB → bootloader →
flash pipeline with a sketch you wrote, *before* Rust enters the picture.
After this part, an upload failure means your firmware, not your board or
cable.

Prerequisites from [Basics.md](Basics.md): toolchain installed (Part 0), and
the reset button's two behaviors (single press = run sketch, double-tap =
bootloader mode).

> **The reset button is the main tool of this entire part.** One button, two
> behaviors — the full background is in Basics.md, but here is what they mean
> operationally, because you will use both in the next few steps:
>
> - **Single press** = reboot and run the sketch. Use it when a sketch seems
>   stuck, or after a successful flash.
> - **Double-tap** = bootloader mode. The LED fades in and out, the sketch
>   does *not* run, and the board sits waiting for firmware over USB. Use it
>   before every upload (Step 1.3c, 1.4c) and whenever the board is invisible
>   (Step 1.2).
> - **After any reset action, the COM port number can change.** Always
>   re-run `arduino-cli board list` afterwards and re-set `$port` if needed.
>
> The failure this prevents is not theoretical: uploading "cold" — while the
> sketch is running — triggers a port re-enumeration race that kills uploads
> mid-write. It produced 9 consecutive upload failures during this course's
> first run. Bootloader mode first, every time.

> **Step 1.1 — Plug the board in**
>
> Connect the UNO R4 WiFi to your PC via USB-C. It must be a **data** cable:
> a charge-only cable powers the board (green ON LED lights, it runs whatever
> sketch is installed) but never appears as a USB device. This exact failure
> happened during this course's first run.

> **Step 1.2 — Find the board and capture its port**
>
> This lists every USB serial device recognized by an installed core:
>
> ```powershell
> arduino-cli board list
> ```
>
> Expected output (verified during this course's first run — compare your
> columns against this):
>
> ```
> Port         Protocol Type              Board Name          FQBN                          Core
> COM3         serial    Serial Port (USB) Arduino UNO R4 WiFi arduino:renesas_uno:unor4wifi arduino:renesas_uno
> ```
>
> If nothing appears: double-tap reset (LED fades = bootloader mode) and run
> it again; then suspect the cable (see Troubleshooting in Basics.md).
>
> Capture the port once, so every later command is copy-paste with no edits:
>
> ```powershell
> $port = 'COM3'   # <- your port from the board list output
> ```
>
> `$port` lives only in this terminal session. New terminal → re-run the line
> (and re-check `board list`: the COM number can change between replugs).

> **Step 1.3 — Flash a sketch you wrote**
>
> An Arduino sketch is a *folder* whose name matches the `.ino` file inside.
> Three steps: create, compile, upload.

> **Step 1.3a — Create the sketch**
>
> Create the folder:
>
> ```powershell
> New-Item -ItemType Directory -Force arduino\sketches\BlinkWipe
> ```
>
> Then create `arduino\sketches\BlinkWipe\BlinkWipe.ino` in any editor and
> type:
>
> ```ino
> void setup() {
>   pinMode(LED_BUILTIN, OUTPUT);   // LED_BUILTIN = D13 on this board
> }
>
> void loop() {
>   digitalWrite(LED_BUILTIN, HIGH);
>   delay(500);
>   digitalWrite(LED_BUILTIN, LOW);
>   delay(500);
> }
> ```
>
> This is C++ for now — it's Arduino's toolchain proving the flashing
> pipeline. From Part 3 on (BlinkEmbedded.md), everything you write is Rust.

> **Step 1.3b — Compile it**
>
> This compiles the sketch for the board identified by the FQBN, leaving the
> output in a new `build\` folder. First compile takes a minute or two:
>
> ```powershell
> arduino-cli compile --fqbn arduino:renesas_uno:unor4wifi --output-dir build arduino\sketches\BlinkWipe
> ```
>
> Expected: ~52 KB / 19% of program storage — that's the C++ blink baseline
> (Arduino core scaffolding included). Your Rust blinky in Part 5
> (BlinkEmbedded.md) will compare against this.

> **Step 1.3c — Upload it**
>
> bossac erases the sketch area, writes the new firmware, and resets the
> board. Bootloader untouched — this is the safe wipe.
>
> ⚠ **Never upload cold.** A cold upload races the port re-enumeration and
> dies mid-write (`SAM-BA operation failed`). Bootloader mode first, every
> time:
>
> **c-1.** Press reset twice, quickly. The board LED fades in and out =
> bootloader mode, waiting for firmware.
>
> **c-2.** Bootloader mode can change the port number — check:
>
> ```powershell
> arduino-cli board list
> ```
>
> If the COM number changed, re-set `$port`.
>
> **c-3.** Upload while the board waits in bootloader mode:
>
> ```powershell
> arduino-cli upload -p $port --fqbn arduino:renesas_uno:unor4wifi --input-dir build
> ```
>
> A rare mid-write `SAM-BA operation failed` can still happen — rerun c-1
> through c-3. Three or more consecutive failures → **DFU escape hatch** in
> Basics.md.

✅ **Checkpoint:** upload reached 100% and the amber LED next to pin 13 blinks
at ~1 Hz.

> **Step 1.4 — Prove the sketch is alive over serial**
>
> A blinking LED is a one-bit output. Text is better. Four steps: edit,
> rebuild, re-upload, listen.

> **Step 1.4a — Add serial output**
>
> Replace the contents of `arduino\sketches\BlinkWipe\BlinkWipe.ino` with:
>
> ```ino
> void setup() {
>   Serial.begin(115200);
>   pinMode(LED_BUILTIN, OUTPUT);
>   Serial.println("BlinkWipe alive");
> }
>
> void loop() {
>   Serial.println("tick");
>   digitalWrite(LED_BUILTIN, HIGH);
>   delay(500);
>   digitalWrite(LED_BUILTIN, LOW);
>   delay(500);
> }
> ```

> **Step 1.4b — Rebuild**
>
> ```powershell
> arduino-cli compile --fqbn arduino:renesas_uno:unor4wifi --output-dir build arduino\sketches\BlinkWipe
> ```

> **Step 1.4c — Re-upload**
>
> ⚠ **Never upload cold** — bootloader mode first (double-tap reset, LED
> fades), then:
>
> ```powershell
> arduino-cli board list
> ```
>
> Re-set `$port` if the COM number changed, then upload:
>
> ```powershell
> arduino-cli upload -p $port --fqbn arduino:renesas_uno:unor4wifi --input-dir build
> ```

> **Step 1.4d — Open the serial monitor**
>
> This connects to `$port` at 115200 baud and prints what the sketch sends
> over USB:
>
> ```powershell
> arduino-cli monitor -p $port --config baudrate=115200
> ```
>
> ✅ **Verified on hardware:** `--config baudrate=115200` connects and streams
> the sketch's output (confirmed this course's first run). Exit with Ctrl+C.
>
> **How to read the result:** `tick` every 500 ms = sketch runs and the
> pipeline is fully proven. No output = the sketch never started — that's a
> flash-image-level problem; report it before proceeding.

✅ **Checkpoint:** `tick` lines streaming in the monitor. Part 1 complete.

---

Next: [BlinkEmbedded.md](BlinkEmbedded.md) — Parts 2–6, where every file you
type is Rust.
