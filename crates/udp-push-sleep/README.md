# udp-push-sleep — the RA4M1 sleeps until a UDP datagram arrives

Embedded Rust port of
[`UDPPushSleep.ino`](https://github.com/TonyMarkham/unor4-wifi-interrupt-bridge/tree/feature/interrupt/examples/UDPPushSleep)
(branch `feature/interrupt`) using `unor4-hal` 0.1.2. **Compiles clean
(`cargo check`, 2026-10-22); not yet run on hardware** — the C++
original ran on the fork's hardware; this port is verified to compile,
not to behave. First-run output gets added here when it exists.

What it does:

1. Joins WiFi over the AT link (SCI1, P501/P502, 115200 8N1) — the same
   sequence as `wifi-sample`, minus the hostname.
2. Opens a UDP socket on port 4210 (`AT+UDPBEGIN=4210`) and switches it
   to push mode (`AT+UDPPUSH=<sock>,1`). A stock bridge answers `ERROR`
   to the push command; the firmware prints `STOPPED:` and halts there.
3. Sleeps. The bridge now sends every datagram for that socket unasked
   as `+UDPRX: <sock>,<len>|<len bytes>\r\n`; the SCI1 receive
   interrupt (`UartIrq`, feature `uart-irq`) wakes the core from
   `wfi()`. Each frame is printed with the count of wake-ups that found
   nothing to read since the previous datagram — 0 means it slept the
   whole time in between.

Differences from the .ino that fall out of the HAL: there is no AGT0
millis timer to stop (the delay is a polled SysTick; nothing runs
periodically), and the receive path has no busy-poll — the RXI handler
fills the ring, the main loop only checks it and sleeps.

Console output goes to **SCI9 (P109/P110)** — the same wire the C++
`Serial` uses under `NO_USB`, forwarded by the bridge to the board's
USB serial port. (`Serial.md` marks this wire as *not yet verified from
Rust*; that verification is part of this crate's first run.)

## Prerequisites

- The interrupt bridge (0.6.101 or later) on the board's ESP32-S3.
- `.env` in this directory: copy `.env.example`, fill in `WIFI_SSID`
  and `WIFI_PASS` (values verbatim, no quotes; 2.4 GHz network).
- `rustup target add thumbv7em-none-eabihf` (already done for the
  course crates).

## Build

> **Step 1 — Change into the package directory**
>
> ```powershell
> Set-Location D:\git\arduino-embedded-rust-poc\crates\udp-push-sleep
> ```

> **Step 2 — Build**
>
> ```powershell
> cargo build --release
> ```
>
> A missing or misspelled `.env` key fails here, at compile time.

> **Step 3 — Convert ELF → raw binary**
>
> ```powershell
> cargo objcopy --release -- -O binary firmware.bin
> ```

## Upload

> ⚠ **Never upload cold** — double-tap reset (LED fades = bootloader
> mode), then confirm the port is still there:
>
> ```powershell
> arduino-cli board list
> ```
>
> ```powershell
> $port = 'COM7'   # <- the UNO R4 WiFi's port from the listing above
> ```
>
> ```powershell
> arduino-cli upload -p $port --fqbn arduino:renesas_uno:unor4wifi --input-file firmware.bin
> ```

✅ **Checkpoint:** upload reports 100%.

## Watch it

> **Step 4 — Open the monitor** (Ctrl+C to close)
>
> ```powershell
> arduino-cli monitor -p $port --config baudrate=115200
> ```

The firmware restarts on upload. Expected shape of the output (exact
captures to be added on the first run):

```
=== UDP push mode, sleeping RA4M1 ===
Joining WiFi <ssid>
> AT+FWVERSION?
< +FWVERSION: <version>
OK

Push mode is ON for socket <n>.
Send a datagram from the PC:
  $bytes = [Text.Encoding]::ASCII.GetBytes("hello"); (New-Object System.Net.Sockets.UdpClient).Send($bytes, $bytes.Length, '<board-ip>', 4210)
Sleeping until a datagram arrives.
```

The join is silent, like the .ino's `WiFi.begin`. Only the three
commands the .ino sends through `atCommand` — `AT+FWVERSION?`,
`AT+UDPBEGIN`, `AT+UDPPUSH` — are echoed: `> cmd` sent, `< reply`
received. `Joined. Board IP: <ip>` appears once the
address is real (never `0.0.0.0`; the firmware waits out DHCP, as the
.ino does).

Then, per datagram sent from the PC:

```
datagram on socket <n>, <len> bytes: <escaped payload>   [idle wake-ups since the previous datagram: 0]
```

A wake-up count of 0 is the point of the exercise: the core slept in
`wfi()` and only the datagram's bytes woke it.

## Troubleshooting

- **`STOPPED: the ESP32-S3 never answered AT`** — bridge firmware not
  running, or the AT link moved. This crate hard-wires `UartIrq` on
  SCI1 (P501/P502); there is no LED fallback, the monitor is the only
  output.
- **`STOPPED: the join failed`** — wrong password, 5 GHz-only network,
  or `.env` values wrapped in quotes.
- **`STOPPED: AT+UDPPUSH was refused`** — the ESP32-S3 runs the stock
  bridge, not the interrupt fork.
- **Garbage characters** — the monitor's baud isn't 115200.
