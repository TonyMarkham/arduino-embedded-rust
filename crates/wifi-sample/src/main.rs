//! wifi-sample — joins the network named in `.env` at boot, then idles.
//!
//! Nothing else: no sockets, no matrix, no serial lesson. The D13 LED is
//! the only status output:
//!   solid on   = joined
//!   fast blink = the ESP32-S3 bridge never answered
//!   slow blink = the bridge answered `AT` but WIFIMODE, BEGINSTA or the
//!                join itself failed
//!
//! SCI1 (P501/P502, board-internal) carries the AT link to the ESP32-S3
//! (see the repo's WiFi.md for the full protocol and its sources). Header
//! D0/D1 are not used.

#![no_std] // don't link the standard library — no OS down here
#![no_main] // no standard main(); we provide our own entry point

use core::fmt::{self, Write};
use embedded_hal::delay::DelayNs;
use panic_halt as _; // crash handler: halt the core on panic
use unor4_hal::{
    Peripherals,
    uart::{ReadError, SciChannel, Uart},
};

// Baked in by build.rs from `.env`.
const WIFI_SSID: &str = env!("WIFI_SSID");
const WIFI_PASS: &str = env!("WIFI_PASS");
const WIFI_HOSTNAME: &str = env!("WIFI_HOSTNAME");

/// "Connected" — the bridge's `WIFI_ST_CONNECTED` (at_handler.h) and the
/// C++ client side's `WL_CONNECTED` (WiFiType.h) share the value 3.
const WL_CONNECTED: u32 = 3;

/// Buffer for one AT reply.
const RESP_MAX: usize = 256;

/// Poll interval while the receiver is empty. One byte at 115200 8N1
/// takes ~87 µs and the SCI holds only one received byte (RDR): a
/// longer sleep lets the next byte overrun the unread one (ORER).
const POLL_US: u32 = 10;

/// Log sink: `writeln!(Console(&mut uart), ...)`. The bridge's parser
/// discards any line not starting with "AT" (and answers ERROR, which
/// `drain` sweeps away) — so logging on this link is free. Never log
/// *between* sending a command and reading its reply.
struct Console<'a, S: SciChannel>(&'a mut Uart<S>);

impl<S: SciChannel> fmt::Write for Console<'_, S> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write_bytes(s.as_bytes());
        Ok(())
    }
}

fn ends_with(buf: &[u8], needle: &[u8]) -> bool {
    buf.len() >= needle.len() && &buf[buf.len() - needle.len()..] == needle
}

/// Copies `parts` back to back into `buf`, truncating at its end.
/// Returns the number of bytes written.
fn concat_into(buf: &mut [u8], parts: &[&[u8]]) -> usize {
    let mut n = 0usize;
    for part in parts {
        for &b in *part {
            if n < buf.len() {
                buf[n] = b;
                n += 1;
            }
        }
    }
    n
}

/// Sends one AT command and collects the reply until `OK\r\n` or
/// `ERROR\r\n`. Returns `(bytes_written, got_ok)`, or `None` on timeout.
/// `read_byte` never blocks: idle waits cost one `POLL_US` delay each.
fn at<S: SciChannel>(
    uart: &mut Uart<S>,
    delay: &mut impl DelayNs,
    cmd: &[u8],
    buf: &mut [u8; RESP_MAX],
    timeout_ms: u32,
) -> Option<(usize, bool)> {
    uart.write_bytes(cmd);
    let mut n = 0usize;
    let timeout_us = timeout_ms.saturating_mul(1_000);
    let mut waited_us = 0u32;
    while waited_us < timeout_us {
        match uart.read_byte() {
            Ok(b) => {
                if n < buf.len() {
                    buf[n] = b;
                    n += 1;
                }
                if ends_with(&buf[..n], b"OK\r\n") {
                    return Some((n, true));
                }
                if ends_with(&buf[..n], b"ERROR\r\n") {
                    return Some((n, false));
                }
            }
            Err(ReadError::Empty) => {
                delay.delay_us(POLL_US);
                waited_us += POLL_US;
            }
            Err(_) => {} // framing/overrun: byte dropped, keep reading
        }
    }
    None
}

/// The value after a `+CMD: ` prompt, up to the line end.
fn after<'a>(reply: &'a [u8], prompt: &[u8]) -> Option<&'a [u8]> {
    if reply.len() < prompt.len() {
        return None;
    }
    for i in 0..=reply.len() - prompt.len() {
        if &reply[i..i + prompt.len()] == prompt {
            let rest = &reply[i + prompt.len()..];
            let end = rest.iter().position(|&b| b == b'\r').unwrap_or(rest.len());
            return Some(&rest[..end]);
        }
    }
    None
}

fn parse_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut v = 0u32;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
    }
    Some(v)
}

/// Discards stale bytes (leftover replies, log-line ERRORs) until the
/// line has been silent for `quiet_ms`.
fn drain<S: SciChannel>(uart: &mut Uart<S>, delay: &mut impl DelayNs, quiet_ms: u32) {
    let quiet_us = quiet_ms.saturating_mul(1_000);
    let mut quiet = 0u32;
    while quiet < quiet_us {
        match uart.read_byte() {
            Ok(_) => quiet = 0,
            Err(ReadError::Empty) => {
                delay.delay_us(POLL_US);
                quiet += POLL_US;
            }
            Err(_) => quiet = 0,
        }
    }
}

// `#[entry]` becomes the function the vector table points to.
// It can never return (`-> !`) — there is nothing to return *to*.
#[cortex_m_rt::entry]
fn main() -> ! {
    // Claim all peripherals exactly once; also initializes the 48 MHz clock.
    let p = Peripherals::take().unwrap();

    let mut delay = p.delay;
    let mut led = p.pins.d13.into_output(); // solid on = joined

    // Boot heartbeat: three quick blips prove the sketch is running.
    // The next LED change is up to ~10 s away, after the bridge
    // handshake succeeds or gives up — don't judge the LED before that.
    for _ in 0..3 {
        led.set_high();
        delay.delay_ms(60);
        led.set_low();
        delay.delay_ms(140);
    }

    // SCI1 — the AT link to the ESP32-S3's bridge firmware, 115200 8N1.
    // TX = P501, RX = P502: board-internal, not header pins (Arduino's
    // WiFiS3 `modem` uses the same pair via Serial2).
    let mut uart = Uart::sci1(p.sci1, p.sci1_tx, p.sci1_rx, 115_200, &p.clocks);
    let mut resp = [0u8; RESP_MAX];

    // Handshake: plain `AT` — the exchange the HAL's slice 6 proved on
    // this board. Any reply (OK or ERROR) means the AT server is up. The
    // ESP32 may still be booting when we arrive, so retry.
    let mut bridge_alive = false;
    for _ in 0..10 {
        drain(&mut uart, &mut delay, 20);
        if at(&mut uart, &mut delay, b"AT\r\n", &mut resp, 700).is_some() {
            bridge_alive = true;
            break;
        }
        delay.delay_ms(300);
    }
    if !bridge_alive {
        loop {
            led.toggle(); // fast blink = bridge never answered
            delay.delay_ms(100);
        }
    }

    // Clear any WiFi state the bridge kept from an earlier sketch.
    // Best-effort, as in WiFiS3's `ModemClass::begin`: up to 3 tries at
    // 500 ms, and the result gates nothing.
    for _ in 0..3 {
        drain(&mut uart, &mut delay, 20);
        if let Some((_, true)) =
            at(&mut uart, &mut delay, b"AT+SOFTRESETWIFI\r\n", &mut resp, 500)
        {
            break;
        }
    }

    // Hostname: the bridge's `+HOSTNAME` only stores the name; the ESP32
    // core applies it when `WiFi.mode()` *changes* to a mode with STA.
    // The bridge may still be in STA from an earlier run, so drop to
    // mode 0 (WIFI_MODE_NULL) first. Best-effort: a rejected name keeps
    // the default `esp32s3-XXXXXX` and the join goes ahead.
    let mut cmd = [0u8; 192];
    drain(&mut uart, &mut delay, 20);
    at(&mut uart, &mut delay, b"AT+WIFIMODE=0\r\n", &mut resp, 2_000);
    let cn = concat_into(
        &mut cmd,
        &[
            b"AT+HOSTNAME=\"".as_slice(),
            WIFI_HOSTNAME.as_bytes(),
            b"\"\r\n".as_slice(),
        ],
    );
    drain(&mut uart, &mut delay, 20);
    at(&mut uart, &mut delay, &cmd[..cn], &mut resp, 1_000);

    // Station mode — the mode change that applies the hostname...
    drain(&mut uart, &mut delay, 20);
    let mut ok = matches!(
        at(&mut uart, &mut delay, b"AT+WIFIMODE=1\r\n", &mut resp, 2_000),
        Some((_, true))
    );

    // ...then join: AT+BEGINSTA="<ssid>","<pass>" — quoted, so spaces
    // (and commas) in the credentials survive the parser.
    let cn = concat_into(
        &mut cmd,
        &[
            b"AT+BEGINSTA=\"".as_slice(),
            WIFI_SSID.as_bytes(),
            b"\",\"".as_slice(),
            WIFI_PASS.as_bytes(),
            b"\"\r\n".as_slice(),
        ],
    );
    if ok {
        drain(&mut uart, &mut delay, 20);
        ok = matches!(
            at(&mut uart, &mut delay, &cmd[..cn], &mut resp, 3_000),
            Some((_, true))
        );
    }

    // Poll the status — the bridge joins in the background and reports
    // progress here. The C++ core polls for up to 10 s; we take 10 s.
    let mut joined = false;
    if ok {
        for _ in 0..20 {
            delay.delay_ms(500);
            drain(&mut uart, &mut delay, 20);
            if let Some((n, true)) =
                at(&mut uart, &mut delay, b"AT+GETSTATUS?\r\n", &mut resp, 1_000)
            {
                if let Some(v) = after(&resp[..n], b"+GETSTATUS: ") {
                    if let Some(s) = parse_u32(v) {
                        if let Ok(txt) = core::str::from_utf8(v) {
                            writeln!(Console(&mut uart), "wifi: status {}", txt).ok();
                        }
                        if s == WL_CONNECTED {
                            joined = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    if joined {
        led.set_high(); // done — this is the whole feature
    } else {
        loop {
            led.toggle(); // slow blink = join failed
            delay.delay_ms(500);
        }
    }

    loop {
        delay.delay_ms(1_000);
    }
}
