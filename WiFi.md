# WiFi — Part 9: join your network, from bare metal

The RA4M1 has no radio. Every WiFi byte this board ever moves goes through
the ESP32-S3, over a UART, using a text command language. This part speaks
that language directly — no library, no stack — and joins your network.

> *Hardware-run on 2026-10-21 as the standalone prototype
> `crates/wifi-sample` — the same `main.rs` as Step 9.3: the D13 LED went
> solid and the board appeared in the router's client list under its
> `WIFI_HOSTNAME`. Steps 9.1–9.7
> in the course crate have not been run as written.*

Prerequisites:
- [Serial.md](Serial.md) — Part 7 complete. This part uses the same
  `Uart` driver, on a different engine: **SCI1** instead of SCI2. The
  USB-UART adapter is not used.
- Your network name and password. The network must be **2.4 GHz** — the
  ESP32-S3 has no 5 GHz radio.

## Where the AT link is

The AT link is **SCI1 on P501 (TX) / P502 (RX)** — board-internal traces
from the RA4M1 to the ESP32-S3, not on any header pin:

- **The C++ core's WiFi modem runs on `Serial2`.** `WiFiS3`'s `Modem.cpp`
  constructs the modem from `&Serial2` for this board.
- **On this board, `Serial2` is P501/P502.** `boards.txt` builds
  `unor4wifi` with `-DNO_USB`. Under `NO_USB`, `Arduino.h` maps `Serial2` →
  `_UART3_`; `SerialObj3.cpp` builds `_UART3_` on pins 24/25, which
  `variants/UNOWIFIR4/variant.cpp` lists as `P501 /* TX WIFI */` and
  `P502 /* RX WIFI */`.
- **The bridge's AT server** is `SERIAL_AT` in `UNOR4USBBridge.ino`:
  `Serial1.begin(115200, SERIAL_8N1, 6, 5)` on the ESP32-S3.
- **P109/P110 is the bridge's data line** (ESP32 UART0, GPIO 44/43) — the
  USB-monitor forwarding path and the upload path. Not involved here.

> [!IMPORTANT]
> `Arduino.h` has **two** serial mappings. The branch *without* `NO_USB`
> maps `Serial2` → `_UART2_` → D1/D0 (SCI2). That branch does not apply to
> the UNO R4 WiFi — reading it puts the AT link on the wrong pins, and the
> bridge never answers.

Consequence: joining WiFi uses `Uart::sci1(p.sci1, p.sci1_tx, p.sci1_rx,
115_200, &p.clocks)`. Header D0/D1 stay free. The conversation runs on
board-internal traces, so there is no wire to tap — the D13 LED is this
part's only status output.

## The wire protocol (all transcribed from source)

**Requests** — one command per line, ended by `\n` (`\r\n` is fine):

| Form | Meaning |
|---|---|
| `AT+CMD\r\n` | run |
| `AT+CMD?\r\n` | read (query a value) |
| `AT+CMD=arg1,arg2\r\n` | write (pass parameters) |

Arguments are split on unquoted commas; `"` toggles quoting (quoted commas
are literal) and `\` escapes the next character. This part quotes the
credentials: `AT+BEGINSTA="ssid","pass"`.

**Responses** — the handler's data first, then the verdict:

```
+CMD: value\r\n
OK\r\n
```

or `ERROR\r\n` (either the command failed or the line was malformed — the
parser discards any line that doesn't start with `AT` and answers `ERROR`).
A bare `AT\r\n` gets a plain `OK\r\n`. The prompt is the command's own name:
`+GETSTATUS: 3\r\nOK\r\n`.

**Receiving with a polled UART** — at 115200 8N1 one byte takes ~87 µs,
and the SCI engine holds exactly one received byte (`RDR`). If a second
byte arrives before the first is read, the overrun flag (`ORER`) sets and
the byte is lost. A receive loop that sleeps 1 ms whenever the line is
empty misses most of every reply — `OK\r\n` (4 bytes, ~350 µs) arrives
while it sleeps, and the handshake times out as if the bridge were dead.
Step 9.3's loops sleep **10 µs** (`POLL_US`) between empty polls, well
under one byte time. (Observed on the 2026-10-21 run: 1 ms → fast blink;
10 µs → joined.)

## The join sequence

1. **Handshake** — `AT\r\n`, retried until any reply (`OK` or `ERROR`)
   arrives. The ESP32 may still be booting when the RA4M1 arrives; the
   firmware tries 10 times, 700 ms each, 300 ms apart.
2. **Soft reset** — `AT+SOFTRESETWIFI\r\n`, best-effort: it clears WiFi
   state the bridge kept from an earlier sketch. WiFiS3's
   `ModemClass::begin` sends it with a 500 ms timeout, up to 3 tries, and
   nothing downstream depends on the result — this firmware does the same.
3. **Hostname** — `AT+WIFIMODE=0\r\n`, then `AT+HOSTNAME="name"\r\n`,
   both best-effort. The hostname is the name your router lists for the
   board; without this step it is the ESP32 core's MAC-based default,
   `esp32s3-XXXXXX`. The bridge's `+HOSTNAME` only *stores* the name — the
   ESP32 core pushes it to the network interface inside `WiFi.mode()`, and
   only when the mode actually changes to one that includes STA. The bridge
   may still be in STA from an earlier run (`+SOFTRESETWIFI` disconnects but
   leaves the mode alone), so the firmware drops to mode `0`
   (`WIFI_MODE_NULL`) first.
4. **Station mode** — `AT+WIFIMODE=1\r\n` (`1` = STA) — the mode change
   that applies the hostname.
5. **Join** — `AT+BEGINSTA="ssid","pass"\r\n`. The bridge starts the join
   and answers immediately with a status code — it does *not* wait.
6. **Poll** — `AT+GETSTATUS?\r\n` every ~500 ms until it reports `3`
   (`WL_CONNECTED`) or ~10 s passes. Values: `0` idle, `1` SSID not found,
   `2` scan completed, `3` connected, `4` connect failed (usually a wrong
   password), `5` connection lost, `6` disconnected.

## Part 9 — The WiFi client

> **Step 9.1 — Create `.env` in the package directory**
>
> Create `crates\arduino-embedded-rust\.env`:
>
> ```text
> # WiFi credentials for Part 9 (WiFi.md). Values are taken verbatim —
> # do not wrap them in quotes. This file contains your network password:
> # keep it out of anything you share or publish.
> WIFI_SSID=your-network-name
> WIFI_PASS=your-network-password
> # The name your router lists for the board (DHCP hostname): letters,
> # digits and hyphens, at most 31 characters.
> WIFI_HOSTNAME=uno-r4-wifi
> ```
>
> Fill in your real values. A password containing `"` or `\` would need the
> protocol's escape rule, which this lesson does not implement — if yours
> has one, change it if you can. The hostname is cut at 31 characters (the
> ESP32 core keeps it in a 32-byte buffer).

**What/why:** a microcontroller has no filesystem to read at runtime — the
credentials and hostname have to be *in* the firmware binary. `.env` keeps them out of
`main.rs` (and out of this repo's docs); a build-script step (next) turns
the file into compile-time constants.

> **Step 9.2 — Extend `build.rs` to bake the credentials in**
>
> Replace `crates\arduino-embedded-rust\build.rs` (the first half is your
> Part 3 build script, unchanged):
>
> ```rust
> //! Feeds `memory.x` to the linker, pulls in cortex-m-rt's `link.x`, and
> //! bakes the WiFi credentials from `.env` into the firmware.
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
>
>     // WiFi credentials: read `.env` (KEY=VALUE lines, # comments) and
>     // expose each key as a compile-time environment variable, so
>     // `env!("WIFI_SSID")` in main.rs embeds the value into the binary.
>     println!("cargo:rerun-if-changed=.env");
>     let env_file = fs::read_to_string(".env").expect("missing .env - see WiFi.md Step 9.1");
>     for line in env_file.lines() {
>         let line = line.trim();
>         if line.is_empty() || line.starts_with('#') {
>             continue;
>         }
>         if let Some((key, value)) = line.split_once('=') {
>             println!("cargo:rustc-env={}={}", key.trim(), value);
>         }
>     }
> }
> ```
>
> **What/why:** `cargo:rustc-env=KEY=VALUE` makes cargo set an environment
> variable *for the Rust code's compilation*, which is what `env!("WIFI_SSID")`
> reads — a compile error if the key is missing, never a silent empty
> string. `rerun-if-changed=.env` makes cargo rebuild when you edit your
> credentials.

> **Step 9.3 — Replace `crates\arduino-embedded-rust\src\main.rs`**
>
> ```rust
> //! Lesson 5: join the WiFi network through the ESP32-S3's AT server.
> //!
> //! Nothing else: no sockets, no matrix, no serial lesson. The D13 LED is
> //! the only status output:
> //!   solid on   = joined
> //!   fast blink = the ESP32-S3 bridge never answered
> //!   slow blink = the bridge answered `AT` but WIFIMODE, BEGINSTA or the
> //!                join itself failed
> //!
> //! SCI1 (P501/P502, board-internal) carries the AT link to the ESP32-S3
> //! (see WiFi.md for the full protocol and its sources). Header
> //! D0/D1 are not used.
>
> #![no_std] // don't link the standard library — no OS down here
> #![no_main] // no standard main(); we provide our own entry point
>
> use core::fmt::{self, Write};
> use embedded_hal::delay::DelayNs;
> use panic_halt as _; // crash handler: halt the core on panic
> use unor4_hal::{
>     Peripherals,
>     uart::{ReadError, SciChannel, Uart},
> };
>
> // Baked in by build.rs from `.env`.
> const WIFI_SSID: &str = env!("WIFI_SSID");
> const WIFI_PASS: &str = env!("WIFI_PASS");
> const WIFI_HOSTNAME: &str = env!("WIFI_HOSTNAME");
>
> /// "Connected" — the bridge's `WIFI_ST_CONNECTED` (at_handler.h) and the
> /// C++ client side's `WL_CONNECTED` (WiFiType.h) share the value 3.
> const WL_CONNECTED: u32 = 3;
>
> /// Buffer for one AT reply.
> const RESP_MAX: usize = 256;
>
> /// Poll interval while the receiver is empty. One byte at 115200 8N1
> /// takes ~87 µs and the SCI holds only one received byte (RDR): a
> /// longer sleep lets the next byte overrun the unread one (ORER).
> const POLL_US: u32 = 10;
>
> /// Log sink: `writeln!(Console(&mut uart), ...)`. The bridge's parser
> /// discards any line not starting with "AT" (and answers ERROR, which
> /// `drain` sweeps away) — so logging on this link is free. Never log
> /// *between* sending a command and reading its reply.
> struct Console<'a, S: SciChannel>(&'a mut Uart<S>);
>
> impl<S: SciChannel> fmt::Write for Console<'_, S> {
>     fn write_str(&mut self, s: &str) -> fmt::Result {
>         self.0.write_bytes(s.as_bytes());
>         Ok(())
>     }
> }
>
> fn ends_with(buf: &[u8], needle: &[u8]) -> bool {
>     buf.len() >= needle.len() && &buf[buf.len() - needle.len()..] == needle
> }
>
> /// Copies `parts` back to back into `buf`, truncating at its end.
> /// Returns the number of bytes written.
> fn concat_into(buf: &mut [u8], parts: &[&[u8]]) -> usize {
>     let mut n = 0usize;
>     for part in parts {
>         for &b in *part {
>             if n < buf.len() {
>                 buf[n] = b;
>                 n += 1;
>             }
>         }
>     }
>     n
> }
>
> /// Sends one AT command and collects the reply until `OK\r\n` or
> /// `ERROR\r\n`. Returns `(bytes_written, got_ok)`, or `None` on timeout.
> /// `read_byte` never blocks: idle waits cost one `POLL_US` delay each.
> fn at<S: SciChannel>(
>     uart: &mut Uart<S>,
>     delay: &mut impl DelayNs,
>     cmd: &[u8],
>     buf: &mut [u8; RESP_MAX],
>     timeout_ms: u32,
> ) -> Option<(usize, bool)> {
>     uart.write_bytes(cmd);
>     let mut n = 0usize;
>     let timeout_us = timeout_ms.saturating_mul(1_000);
>     let mut waited_us = 0u32;
>     while waited_us < timeout_us {
>         match uart.read_byte() {
>             Ok(b) => {
>                 if n < buf.len() {
>                     buf[n] = b;
>                     n += 1;
>                 }
>                 if ends_with(&buf[..n], b"OK\r\n") {
>                     return Some((n, true));
>                 }
>                 if ends_with(&buf[..n], b"ERROR\r\n") {
>                     return Some((n, false));
>                 }
>             }
>             Err(ReadError::Empty) => {
>                 delay.delay_us(POLL_US);
>                 waited_us += POLL_US;
>             }
>             Err(_) => {} // framing/overrun: byte dropped, keep reading
>         }
>     }
>     None
> }
>
> /// The value after a `+CMD: ` prompt, up to the line end.
> fn after<'a>(reply: &'a [u8], prompt: &[u8]) -> Option<&'a [u8]> {
>     if reply.len() < prompt.len() {
>         return None;
>     }
>     for i in 0..=reply.len() - prompt.len() {
>         if &reply[i..i + prompt.len()] == prompt {
>             let rest = &reply[i + prompt.len()..];
>             let end = rest.iter().position(|&b| b == b'\r').unwrap_or(rest.len());
>             return Some(&rest[..end]);
>         }
>     }
>     None
> }
>
> fn parse_u32(bytes: &[u8]) -> Option<u32> {
>     if bytes.is_empty() {
>         return None;
>     }
>     let mut v = 0u32;
>     for &b in bytes {
>         if !b.is_ascii_digit() {
>             return None;
>         }
>         v = v.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
>     }
>     Some(v)
> }
>
> /// Discards stale bytes (leftover replies, log-line ERRORs) until the
> /// line has been silent for `quiet_ms`.
> fn drain<S: SciChannel>(uart: &mut Uart<S>, delay: &mut impl DelayNs, quiet_ms: u32) {
>     let quiet_us = quiet_ms.saturating_mul(1_000);
>     let mut quiet = 0u32;
>     while quiet < quiet_us {
>         match uart.read_byte() {
>             Ok(_) => quiet = 0,
>             Err(ReadError::Empty) => {
>                 delay.delay_us(POLL_US);
>                 quiet += POLL_US;
>             }
>             Err(_) => quiet = 0,
>         }
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
>     let mut delay = p.delay;
>     let mut led = p.pins.d13.into_output(); // solid on = joined
>
>     // Boot heartbeat: three quick blips prove the sketch is running.
>     // The next LED change is up to ~10 s away, after the bridge
>     // handshake succeeds or gives up — don't judge the LED before that.
>     for _ in 0..3 {
>         led.set_high();
>         delay.delay_ms(60);
>         led.set_low();
>         delay.delay_ms(140);
>     }
>
>     // SCI1 — the AT link to the ESP32-S3's bridge firmware, 115200 8N1.
>     // TX = P501, RX = P502: board-internal, not header pins (Arduino's
>     // WiFiS3 `modem` uses the same pair via Serial2).
>     let mut uart = Uart::sci1(p.sci1, p.sci1_tx, p.sci1_rx, 115_200, &p.clocks);
>     let mut resp = [0u8; RESP_MAX];
>
>     // Handshake: plain `AT` — the exchange the HAL's slice 6 proved on
>     // this board. Any reply (OK or ERROR) means the AT server is up. The
>     // ESP32 may still be booting when we arrive, so retry.
>     let mut bridge_alive = false;
>     for _ in 0..10 {
>         drain(&mut uart, &mut delay, 20);
>         if at(&mut uart, &mut delay, b"AT\r\n", &mut resp, 700).is_some() {
>             bridge_alive = true;
>             break;
>         }
>         delay.delay_ms(300);
>     }
>     if !bridge_alive {
>         loop {
>             led.toggle(); // fast blink = bridge never answered
>             delay.delay_ms(100);
>         }
>     }
>
>     // Clear any WiFi state the bridge kept from an earlier sketch.
>     // Best-effort, as in WiFiS3's `ModemClass::begin`: up to 3 tries at
>     // 500 ms, and the result gates nothing.
>     for _ in 0..3 {
>         drain(&mut uart, &mut delay, 20);
>         if let Some((_, true)) =
>             at(&mut uart, &mut delay, b"AT+SOFTRESETWIFI\r\n", &mut resp, 500)
>         {
>             break;
>         }
>     }
>
>     // Hostname: the bridge's `+HOSTNAME` only stores the name; the ESP32
>     // core applies it when `WiFi.mode()` *changes* to a mode with STA.
>     // The bridge may still be in STA from an earlier run, so drop to
>     // mode 0 (WIFI_MODE_NULL) first. Best-effort: a rejected name keeps
>     // the default `esp32s3-XXXXXX` and the join goes ahead.
>     let mut cmd = [0u8; 192];
>     drain(&mut uart, &mut delay, 20);
>     at(&mut uart, &mut delay, b"AT+WIFIMODE=0\r\n", &mut resp, 2_000);
>     let cn = concat_into(
>         &mut cmd,
>         &[
>             b"AT+HOSTNAME=\"".as_slice(),
>             WIFI_HOSTNAME.as_bytes(),
>             b"\"\r\n".as_slice(),
>         ],
>     );
>     drain(&mut uart, &mut delay, 20);
>     at(&mut uart, &mut delay, &cmd[..cn], &mut resp, 1_000);
>
>     // Station mode — the mode change that applies the hostname...
>     drain(&mut uart, &mut delay, 20);
>     let mut ok = matches!(
>         at(&mut uart, &mut delay, b"AT+WIFIMODE=1\r\n", &mut resp, 2_000),
>         Some((_, true))
>     );
>
>     // ...then join: AT+BEGINSTA="<ssid>","<pass>" — quoted, so spaces
>     // (and commas) in the credentials survive the parser.
>     let cn = concat_into(
>         &mut cmd,
>         &[
>             b"AT+BEGINSTA=\"".as_slice(),
>             WIFI_SSID.as_bytes(),
>             b"\",\"".as_slice(),
>             WIFI_PASS.as_bytes(),
>             b"\"\r\n".as_slice(),
>         ],
>     );
>     if ok {
>         drain(&mut uart, &mut delay, 20);
>         ok = matches!(
>             at(&mut uart, &mut delay, &cmd[..cn], &mut resp, 3_000),
>             Some((_, true))
>         );
>     }
>
>     // Poll the status — the bridge joins in the background and reports
>     // progress here. The C++ core polls for up to 10 s; we take 10 s.
>     let mut joined = false;
>     if ok {
>         for _ in 0..20 {
>             delay.delay_ms(500);
>             drain(&mut uart, &mut delay, 20);
>             if let Some((n, true)) =
>                 at(&mut uart, &mut delay, b"AT+GETSTATUS?\r\n", &mut resp, 1_000)
>             {
>                 if let Some(v) = after(&resp[..n], b"+GETSTATUS: ") {
>                     if let Some(s) = parse_u32(v) {
>                         if let Ok(txt) = core::str::from_utf8(v) {
>                             writeln!(Console(&mut uart), "wifi: status {}", txt).ok();
>                         }
>                         if s == WL_CONNECTED {
>                             joined = true;
>                             break;
>                         }
>                     }
>                 }
>             }
>         }
>     }
>
>     if joined {
>         led.set_high(); // done — this is the whole feature
>     } else {
>         loop {
>             led.toggle(); // slow blink = join failed
>             delay.delay_ms(500);
>         }
>     }
>
>     loop {
>         delay.delay_ms(1_000);
>     }
> }
> ```
>
> **What/why:** `Uart::sci1` claims SCI1 with the two board-internal pins
> `p.sci1_tx` (P501) and `p.sci1_rx` (P502). `Uart` without a type
> parameter means `Uart<SCI2>`, so `Console`, `at`, and `drain` are generic
> over `S: SciChannel` — the HAL's trait for its SCI engines — to accept the
> SCI1 driver. `POLL_US` is the receive-loop interval explained in "The wire
> protocol" above. `concat_into` assembles the two commands that carry
> `.env` values (`+HOSTNAME`, `+BEGINSTA`) into one reused buffer. The
> hostname steps are best-effort: if the bridge rejects the name, the board
> joins under its default `esp32s3-XXXXXX`. The `wifi: status N` log
> lines go to the AT server, which discards them (and answers `ERROR`,
> which `drain` sweeps away) — they cost nothing, but nothing outside the
> board can see them.

| D13 pattern | Meaning |
|---|---|
| three quick blips at boot | firmware is alive (then dark for up to ~10 s during the handshake) |
| solid on | joined |
| fast blink | the ESP32-S3 never answered `AT` |
| slow blink | the bridge answered `AT`, but `+WIFIMODE` / `+BEGINSTA` was rejected or the join failed |

> **Step 9.4a — Change into the package directory**
>
> `.cargo/config.toml` there pins the target:
>
> ```powershell
> Set-Location crates\arduino-embedded-rust
> ```

> **Step 9.4b — Build**
>
> ```powershell
> cargo build --release
> ```
>
> A missing or misspelled `.env` key fails here, at compile time — that is
> `env!` doing its job. Optional, if you want the size: `cargo size --release`.

> **Step 9.5 — Convert ELF → raw binary**
>
> ```powershell
> cargo objcopy --release -- -O binary firmware.bin
> ```

> **Step 9.6 — Upload**
>
> ⚠ **Never upload cold** — double-tap reset (LED fades = bootloader
> mode), then confirm the port is still there:
>
> ```powershell
> arduino-cli board list
> ```
>
> Re-set `$port` to the `UNO R4 WiFi` line's port if the number changed:
>
> ```powershell
> $port = 'COM7'   # <- your board's port from the listing above
> ```
>
> Then upload (from the package directory, where `firmware.bin` lives):
>
> ```powershell
> arduino-cli upload -p $port --fqbn arduino:renesas_uno:unor4wifi --input-file firmware.bin
> ```

✅ **Checkpoint:** upload reports 100%.

---

> **Step 9.7 — Check the join**
>
> The firmware starts on its own after the upload. Watch D13: three quick
> blips, up to ~10 s dark, then the pattern from the table above. Once it
> is solid, open your router's list of connected clients (its admin page
> or app).
>
> Expected (2026-10-21 run): D13 solid, and the router's list shows a
> client named after your `WIFI_HOSTNAME`.

✅ **Checkpoint:** D13 is solid and the board appears in your router's
client list under your `WIFI_HOSTNAME`. Your bare-metal firmware is on
your network — every packet it will ever receive arrives through the
protocol you just implemented by hand.

---

## Troubleshooting

- **Compile error: `environment variable not defined: WIFI_SSID` (or
  `WIFI_PASS`, `WIFI_HOSTNAME`)** — the key is missing or misspelled in
  `.env`, or Step 9.2 wasn't applied. The build script's panic (`missing .env`) means the file
  isn't in `crates\arduino-embedded-rust\` next to `Cargo.toml`.
- **Fast blink, forever (the ESP32 never answered `AT`)** — check that
  `firmware.bin` really uploaded (a fresh build, then Step 9.5 again before
  9.6). Check that the UART is `Uart::sci1` on `p.sci1_tx` / `p.sci1_rx` —
  SCI2 on D0/D1 is not the AT link. Check that `at` and
  `drain` sleep `delay_us(POLL_US)`, not `delay_ms(1)`, between empty
  polls: the 1 ms version produces exactly this symptom.
- **Slow blink, forever (join failed)** — the SSID wasn't found (range, or
  a 5 GHz-only network), the password is wrong, or `.env` wraps a value in
  quotes (values are taken verbatim; `main.rs` adds the quotes itself). The
  network must be 2.4 GHz.
- **Joined, but the router still lists `esp32s3-XXXXXX`** — the router may
  be showing the name from the old address lease: unplug and replug the
  board's USB cable, or wait for the lease to renew. If the default name
  persists, check that `.env`'s `WIFI_HOSTNAME` uses only letters, digits
  and hyphens, and that `main.rs` sends `AT+WIFIMODE=0` *before*
  `+HOSTNAME` and `AT+WIFIMODE=1` after it.
- **Build succeeds, upload fails** — unrelated to this part:
  never-upload-cold (Step 9.6), then the DFU escape hatch in Basics.md.

## Where every fact above came from

- **ArduinoCore-renesas 1.6.0, `boards.txt`** — `unor4wifi.build.defines`
  includes `-DNO_USB`.
- **ArduinoCore-renesas 1.6.0, `cores/arduino/Arduino.h` + `Serial.h` +
  `SerialObj3.cpp`, `variants/UNOWIFIR4/pins_arduino.h` + `variant.cpp`** —
  under `NO_USB`: `Serial` = `_UART1_` (pins 22/23 = P109/P110),
  `Serial1` = `_UART2_` (pins 1/0 = D1/D0 = P302/P301), `Serial2` =
  `_UART3_` (pins 24/25 = P501 "TX WIFI" / P502 "RX WIFI").
- **ArduinoCore-renesas 1.6.0, `libraries/WiFiS3/src/Modem.cpp` +
  `Modem.h` + `WiFiCommands.h`** — the client: modem on `&Serial2` for
  `ARDUINO_UNOWIFIR4`, 115200 default, commands built as `"AT" x "\r\n"`,
  the `+SOFTRESETWIFI` handshake (500 ms timeout, 3 retries, result kept in
  `beginned`, which only decides whether `begin` runs again), the 10 s join
  poll, the response grammar (`+CMD: value` then `OK`/`ERROR`), and the
  10 000 ms modem timeout. `WiFi.cpp`: `setHostname` sends
  `AT+HOSTNAME=<name>`.
- **uno-r4-wifi-usb-bridge, `UNOR4USBBridge.ino`** — `SERIAL_AT` =
  `Serial1` at 115200 8N1 on ESP32 GPIOs 6/5; the bridge data line on
  GPIO 44/43 (→ P109/P110); the AT server task started at boot.
- **uno-r4-wifi-usb-bridge, `commands.h`, `at_handler.h`,
  `cmds_wifi_station.h`, `cmds_esp_generic.h`, `chAT.hpp`, `parser.cpp`,
  `server.cpp`** — command names (`+SOFTRESETWIFI`, `+WIFIMODE`,
  `+HOSTNAME`, `+BEGINSTA`, `+GETSTATUS`), the parser grammar (quoting,
  escaping, `\n` line end), the response bytes (`+CMD: `, `OK\r\n`,
  `ERROR\r\n`), and `WIFI_ST_CONNECTED = 3`. `+WIFIMODE=n` maps `0`/`1`/`2`/`3`
  to `WiFi.mode(WIFI_MODE_NULL / STA / AP / APSTA)`; `+HOSTNAME=<name>`
  calls `WiFi.setHostname(name)` (`ERROR` on an empty name or wrong argument
  count); `+SOFTRESETWIFI` disconnects STA and AP but does not change the
  mode.
- **arduino-esp32 2.0.9 (upstream tag), `libraries/WiFi/src/WiFiGeneric.cpp`**
  — `setHostname` only fills a 32-byte `default_hostname` buffer (default:
  `esp32s3-` + three MAC bytes); `mode()` pushes it to the STA interface
  only when the mode changes to one that includes STA. Read from the
  upstream tag, not Arduino's `unor4wifi-2.0.9` fork; the 2026-10-21 run
  (router listed the new name) confirms the behavior on this board.
- **arduino-esp32 (unor4wifi-2.0.9), `libraries/WiFi/src/WiFiType.h`** —
  the `wl_status_t` values the client side compares against.
- **`unor4-hal` 0.1.2 source** — `Uart::sci1(sci, tx: Pin<'5', 1, Input>,
  rx: Pin<'5', 2, Input>, baud, clocks)` and the `sci1_tx` / `sci1_rx`
  fields of `Peripherals`; `SciChannel` exported from `uart`; the polled
  `read_byte` (non-FIFO receive, `ORER` reported as `ReadError::Overrun`
  with the byte dropped). Its README records SCI1 answering `AT` → `OK`
  on hardware (2026-10-19).
