# wifi-sample

Prototype, one feature: at boot it joins the WiFi network named in `.env`
under the hostname `WIFI_HOSTNAME` (the name your router lists), then idles.
Detached from the course workspace on purpose.

## Status LED (D13)

| Pattern | Meaning |
|---|---|
| three quick blips at boot | sketch is alive (then dark for up to ~10 s during the bridge handshake) |
| solid on | joined |
| fast blink | ESP32-S3 bridge never answered `AT` |
| slow blink | bridge answered `AT`, but `+WIFIMODE` / `+BEGINSTA` was rejected or the join failed (wrong password / SSID not found / not a 2.4 GHz network) |

The AT conversation runs on **SCI1** — RA4M1 **P501** (TX) / **P502**
(RX), board-internal traces to the ESP32-S3, not on any header pin.
Header D0/D1 are not used by this firmware. The `wifi: status N` log
lines share that link (the AT server discards them), so the LED is the
only status you can observe.

Source (ArduinoCore-renesas 1.6.0): `boards.txt` builds `unor4wifi` with
`-DNO_USB`, under which `Arduino.h` maps `Serial2` → `_UART3_`;
`SerialObj3.cpp` builds `_UART3_` on pins 24/25, which
`variants/UNOWIFIR4/variant.cpp` lists as P501 "TX WIFI" / P502 "RX WIFI";
`WiFiS3`'s `Modem.cpp` opens the modem on `&Serial2`.

## Build and flash

Prereqs: the course's Part 0 toolchain (incl.
`rustup target add thumbv7em-none-eabihf` and `cargo binutils`), and the
board's bootloader untouched.

> **1 — Fill in the credentials and hostname**
>
> ```powershell
> Copy-Item .env.example .env   # if starting fresh; then edit values
> ```
>
> `.env` needs three keys: `WIFI_SSID`, `WIFI_PASS`, and `WIFI_HOSTNAME`
> (letters, digits and hyphens, at most 31 characters). A missing key is a
> compile error. Values are taken verbatim — no quotes.

> **2 — Build**
>
> ```powershell
> cargo build --release
> ```

> **3 — Convert ELF → raw binary**
>
> ```powershell
> cargo objcopy --release -- -O binary firmware.bin
> ```

> **4 — Find the board's port and capture it as `$port`**
>
> Plug the board in by USB and list the serial ports:
>
> ```powershell
> arduino-cli board list
> ```
>
> Find the line whose *FQBN* (or board name) says `UNO R4 WiFi` — its
> port is the first column, e.g. `COM7`. Capture it for the next step:
>
> ```powershell
> $port = 'COM7'   # <- your board's port from the listing above
> ```
>
> This variable only lives in your terminal session — if you open a new
> one (or the board re-enumerates to a different COM number), re-run both
> commands.

> **5 — Upload**
>
> ⚠ **Never upload cold** — double-tap reset (LED fades = bootloader
> mode), then confirm the port is still there and re-set `$port` if it
> changed:
>
> ```powershell
> arduino-cli board list
> ```
>
> ```powershell
> $port = 'COM7'   # <- re-set if the number changed
> ```
>
> Then upload (from this directory, where `firmware.bin` lives):
>
> ```powershell
> arduino-cli upload -p $port --fqbn arduino:renesas_uno:unor4wifi --input-file firmware.bin
> ```

Repeated mid-write `SAM-BA operation failed` → the DFU escape hatch in the
repo's Basics.md.

After the upload, the sketch starts on its own: the D13 LED goes solid
once the board has joined your network (LED patterns above).
