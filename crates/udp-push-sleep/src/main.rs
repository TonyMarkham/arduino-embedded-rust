//! udp-push-sleep — the RA4M1 sleeps until a UDP datagram arrives.
//!
//! Embedded Rust port of `UDPPushSleep.ino` from the interrupt bridge
//! (https://github.com/TonyMarkham/unor4-wifi-interrupt-bridge, branch
//! `feature/interrupt`, examples/UDPPushSleep). Needs that bridge
//! (0.6.101 or later) on the board's ESP32-S3.
//!
//! setup joins WiFi over the AT link (SCI1, P501/P502), then switches a
//! UDP socket to push mode with AT+UDPPUSH: from then on the bridge
//! sends every datagram for that socket unasked, as
//!
//!   +UDPRX: <sock>,<len>|<len bytes>\r\n
//!
//! and the SCI1 receive interrupt wakes the core from `wfi()`. Console
//! output goes to SCI9 (P109/P110) — the same line the C++ sketch's
//! `Serial` writes under NO_USB; read it with `arduino-cli monitor`.
//! Each frame is printed with the number of times the core woke up with
//! nothing to read since the previous datagram. A count of 0 shows it
//! slept the whole time in between.
//!
//! Differences from the .ino that fall out of the HAL, not choices:
//! there is no AGT0/millis timer to stop — unor4-hal's delay is a
//! polled SysTick and nothing else runs periodically, so nothing can
//! wake the core except the SCI1 events (RXI/ERI). And the receive
//! path needs no interrupt-masking dance of its own: `UartIrq`'s RXI
//! handler drains the SCI into a ring, so the loop's only job is to
//! check the ring and sleep.
//!
//! UNVERIFIED on hardware until the first run. The C++ original was
//! run on the fork's hardware; this file has only been read.

#![no_std] // don't link the standard library — no OS down here
#![no_main] // no standard main(); we provide our own entry point

use core::fmt::{self, Write};
use embedded_hal::delay::DelayNs;
use panic_halt as _; // crash handler: halt the core on panic
use unor4_hal::{
    Peripherals,
    uart::{SciChannel, Uart, UartIrq},
};

// Baked in by build.rs from `.env`.
const WIFI_SSID: &str = env!("WIFI_SSID");
const WIFI_PASS: &str = env!("WIFI_PASS");

/// The UDP port the bridge listens on (the .ino's UDP_PORT).
const UDP_PORT: u32 = 4210;

/// Largest datagram the bridge forwards (WiFiUDP::parsePacket's buffer).
const MAX_PAYLOAD: usize = 1460;

/// Buffer for one AT reply.
const RESP_MAX: usize = 256;

/// "Connected" — the bridge's `WIFI_ST_CONNECTED` (at_handler.h) and the
/// C++ client side's `WL_CONNECTED` (WiFiType.h) share the value 3.
const WL_CONNECTED: u32 = 3;

/// The AT link, interrupt-driven: the RXI handler fills the receive
/// ring, the TXI handler drains the transmit one, and any of the
/// engine's events wakes the core out of `wfi()`. A `static` because
/// the handlers must reach it after `sci1` returns.
static AT_LINK: UartIrq<ra4m1::SCI1> = UartIrq::new();

/// Console sink: `writeln!(Console(uart), ...)` on the monitor line.
/// The bridge forwards these bytes to the USB serial monitor.
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

/// Writes `v` as decimal ASCII into `buf`, returns the used slice.
fn u32_ascii(mut v: u32, buf: &mut [u8; 10]) -> &[u8] {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    &buf[i..]
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

/// Discards stale bytes until the line has been quiet for `quiet_ms`.
/// With the IRQ driver the ring can never overrun, so waiting 1 ms per
/// empty check is safe — wifi-sample's 10 µs polling dance existed only
/// because the polling driver holds one received byte.
fn drain(delay: &mut impl DelayNs, quiet_ms: u32) {
    let mut quiet = 0u32;
    while quiet < quiet_ms {
        if AT_LINK.read_byte().is_some() {
            quiet = 0;
        } else {
            delay.delay_ms(1);
            quiet += 1;
        }
    }
}

/// Sends one AT command and collects the reply until `OK\r\n` or
/// `ERROR\r\n`. Returns `(bytes_written, got_ok)`, or `None` on
/// timeout. The timeout counts only while the line is idle: bytes
/// arriving means the reply is still coming. Silent — the join steps
/// stand in for WiFiS3's `WiFi.begin`, which prints nothing.
fn at(
    delay: &mut impl DelayNs,
    cmd: &[u8],
    buf: &mut [u8; RESP_MAX],
    timeout_ms: u32,
) -> Option<(usize, bool)> {
    AT_LINK.write_all(cmd);
    let mut n = 0usize;
    let mut waited_ms = 0u32;
    while waited_ms < timeout_ms {
        match AT_LINK.read_byte() {
            Some(b) => {
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
            None => {
                delay.delay_ms(1);
                waited_ms += 1;
            }
        }
    }
    None
}

/// `at`, with the conversation echoed to the console — `> cmd` before,
/// `< reply` after — exactly what the .ino's `atCommand` prints. Used
/// only for the commands the .ino sends through `atCommand`.
fn at_echo(
    console: &mut Uart<ra4m1::SCI9>,
    delay: &mut impl DelayNs,
    cmd: &[u8],
    buf: &mut [u8; RESP_MAX],
    timeout_ms: u32,
) -> Option<(usize, bool)> {
    // The command text without its trailing "\r\n".
    write!(Console(console), "> ").ok();
    console.write_bytes(&cmd[..cmd.len().saturating_sub(2)]);
    writeln!(Console(console)).ok();

    let outcome = at(delay, cmd, buf, timeout_ms);

    // "< " then the reply: `\n` becomes a line break, `\r` skipped,
    // everything else escaped — the .ino's reply print, byte for byte.
    write!(Console(console), "< ").ok();
    match outcome {
        Some((n, _)) => {
            for &b in &buf[..n] {
                if b == b'\n' {
                    writeln!(Console(console)).ok();
                } else if b != b'\r' {
                    print_escaped(console, b);
                }
            }
        }
        None => {
            console.write_bytes(b"(timeout)");
            writeln!(Console(console)).ok();
        }
    }
    outcome
}

/// Prints why and stops (the .ino's `halt`).
fn halt<S: SciChannel>(uart: &mut Uart<S>, why: &str) -> ! {
    writeln!(Console(uart), "STOPPED: {}", why).ok();
    loop {
        cortex_m::asm::wfi(); // halt asleep, not spinning
    }
}

/// One byte, readably: printable ASCII as-is, anything else as \xHH
/// (the .ino's `printEscapedByte`).
fn print_escaped<S: SciChannel>(uart: &mut Uart<S>, b: u8) {
    let mut buf = [0u8; 4];
    let shown: &[u8] = if (0x20..0x7f).contains(&b) {
        buf[0] = b;
        &buf[..1]
    } else {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        buf[0] = b'\\';
        buf[1] = b'x';
        buf[2] = HEX[usize::from(b >> 4)];
        buf[3] = HEX[usize::from(b & 0xf)];
        &buf
    };
    if let Ok(txt) = core::str::from_utf8(shown) {
        uart.write_bytes(txt.as_bytes());
    }
}

// ---- +UDPRX: frame parser, fed one byte at a time from the main loop ----

enum ParseState {
    Header,
    Payload,
    Trailer,
}

/// The .ino's parser state, as a value instead of file-scope statics.
struct FrameParser {
    state: ParseState,
    header: [u8; 32], // "+UDPRX: <sock>,<len>" while it arrives
    header_len: usize,
    frame_sock: u32,
    frame_len: usize, // payload length announced by the header
    payload: [u8; MAX_PAYLOAD],
    payload_got: usize, // payload bytes received so far
    trailer_got: usize,
}

impl FrameParser {
    const fn new() -> Self {
        Self {
            state: ParseState::Header,
            header: [0; 32],
            header_len: 0,
            frame_sock: 0,
            frame_len: 0,
            payload: [0; MAX_PAYLOAD],
            payload_got: 0,
            trailer_got: 0,
        }
    }

    /// Consumes one byte; returns `(sock, announced_len)` once a whole
    /// frame — payload plus the trailing "\r\n" — has arrived.
    fn parse_byte(&mut self, c: u8) -> Option<(u32, usize)> {
        match self.state {
            ParseState::Header => {
                if c == b'|' {
                    if let Some((sock, len)) = parse_frame_header(&self.header[..self.header_len]) {
                        self.frame_sock = sock;
                        self.frame_len = len;
                        self.payload_got = 0;
                        self.trailer_got = 0;
                        self.state = if len > 0 {
                            ParseState::Payload
                        } else {
                            ParseState::Trailer
                        };
                    }
                    self.header_len = 0;
                } else if c == b'\n' {
                    // a line that is not a frame header: not expected
                    // after setup
                    self.header_len = 0;
                } else if self.header_len < self.header.len() {
                    self.header[self.header_len] = c;
                    self.header_len += 1;
                } else {
                    self.header_len = 0; // too long to be a header: start over
                }
                None
            }
            ParseState::Payload => {
                if self.payload_got < MAX_PAYLOAD {
                    self.payload[self.payload_got] = c;
                }
                self.payload_got += 1;
                if self.payload_got == self.frame_len {
                    self.state = ParseState::Trailer;
                    self.trailer_got = 0;
                }
                None
            }
            ParseState::Trailer => {
                // the "\r\n" that ends the frame
                self.trailer_got += 1;
                if self.trailer_got == 2 {
                    self.state = ParseState::Header;
                    Some((self.frame_sock, self.frame_len))
                } else {
                    None
                }
            }
        }
    }
}

/// The .ino's `sscanf(header, "+UDPRX: %d,%u", ...)` in Rust.
fn parse_frame_header(buf: &[u8]) -> Option<(u32, usize)> {
    const PREFIX: &[u8] = b"+UDPRX: ";
    if buf.len() < PREFIX.len() || &buf[..PREFIX.len()] != PREFIX {
        return None;
    }
    let rest = &buf[PREFIX.len()..];
    let comma = rest.iter().position(|&b| b == b',')?;
    let sock = parse_u32(&rest[..comma])?;
    let len = parse_u32(&rest[comma + 1..])?;
    Some((sock, len as usize))
}

/// Prints one received datagram and the idle wake-up count (the .ino's
/// `reportDatagram`). No flush needed before sleeping: SCI9 runs in
/// polling mode with no interrupts enabled, so its transmit can never
/// wake the core.
fn report<S: SciChannel>(uart: &mut Uart<S>, parser: &FrameParser, idle_wakeups: u32) {
    let shown = parser.frame_len.min(MAX_PAYLOAD);
    write!(
        Console(uart),
        "datagram on socket {}, {} bytes: ",
        parser.frame_sock,
        parser.frame_len
    )
    .ok();
    for &b in &parser.payload[..shown] {
        print_escaped(uart, b);
    }
    if shown < parser.frame_len {
        uart.write_bytes(b" (truncated)");
    }
    writeln!(
        Console(uart),
        "   [idle wake-ups since the previous datagram: {}]",
        idle_wakeups
    )
    .ok();
}

// `#[entry]` becomes the function the vector table points to.
// It can never return (`-> !`) — there is nothing to return *to*.
#[cortex_m_rt::entry]
fn main() -> ! {
    // Claim all peripherals exactly once; also initializes the 48 MHz clock.
    let p = Peripherals::take().unwrap();

    let mut delay = p.delay;

    // Console on SCI9 (P109/P110) — the monitor line. Board-internal:
    // the ESP32-S3 forwards it to the USB serial monitor, which is what
    // the C++ sketch's `Serial` reaches under NO_USB.
    let mut console = Uart::sci9(p.sci9, p.sci9_tx, p.sci9_rx, 115_200, &p.clocks);

    // Attach the AT link: SCI1 on the board-internal P501 (TX) / P502
    // (RX), 115200 8N1 — the same pair WiFiS3's `modem` uses. The attach
    // wires RXI/TXI/ERI through IELSR + NVIC and turns receive
    // interrupts on; from here on every incoming byte lands in the ring.
    AT_LINK.sci1(p.sci1, p.sci1_tx, p.sci1_rx, 115_200, &p.clocks);
    // Unmask the CPU after the bootloader — the attach contract's last step.
    unsafe { unor4_hal::intc::enable_interrupts() };

    writeln!(
        Console(&mut console),
        "\r\n=== UDP push mode, sleeping RA4M1 ==="
    )
    .ok();
    writeln!(Console(&mut console), "Joining WiFi {}", WIFI_SSID).ok();

    let mut resp = [0u8; RESP_MAX];

    // Handshake: plain `AT`, retried while the ESP32 may still be
    // booting. `drain` first — push mode survives an RA4M1 reset, so a
    // stray `+UDPRX:` frame from an earlier sketch must not count as
    // the handshake reply.
    let mut bridge_alive = false;
    for _ in 0..10 {
        drain(&mut delay, 20);
        if at(&mut delay, b"AT\r\n", &mut resp, 700).is_some() {
            bridge_alive = true;
            break;
        }
        delay.delay_ms(300);
    }
    if !bridge_alive {
        halt(&mut console, "the ESP32-S3 never answered AT");
    }

    // Clear WiFi state the bridge kept from an earlier sketch — and
    // with it every UDP socket and push flag (`+SOFTRESETWIFI` deletes
    // them all). Best-effort, as in WiFiS3's `ModemClass::begin`.
    for _ in 0..3 {
        drain(&mut delay, 20);
        if let Some((_, true)) = at(&mut delay, b"AT+SOFTRESETWIFI\r\n", &mut resp, 500) {
            break;
        }
    }

    // Join, retrying forever — the .ino's
    // `while (WiFi.begin(...) != WL_CONNECTED)` loop with its
    // "not joined yet, retrying in 5 s". WiFiS3's `begin` sets mode
    // STA, sends BEGINSTA, then polls the status; each pass here does
    // the same: `AT+WIFIMODE=1`, `AT+BEGINSTA=...`, status poll.
    let mut cmd = [0u8; 192];
    let cn = concat_into(
        &mut cmd,
        &[
            b"AT+BEGINSTA=\"".as_slice(),
            WIFI_SSID.as_bytes(),
            b"\",\"",
            WIFI_PASS.as_bytes(),
            b"\"\r\n".as_slice(),
        ],
    );
    loop {
        drain(&mut delay, 20);
        at(&mut delay, b"AT+WIFIMODE=1\r\n", &mut resp, 2_000);
        drain(&mut delay, 20);
        let ok = matches!(
            at(&mut delay, &cmd[..cn], &mut resp, 3_000),
            Some((_, true))
        );
        let mut joined = false;
        if ok {
            for _ in 0..20 {
                delay.delay_ms(500);
                drain(&mut delay, 20);
                if let Some((n, true)) = at(&mut delay, b"AT+GETSTATUS?\r\n", &mut resp, 1_000)
                    && let Some(v) = after(&resp[..n], b"+GETSTATUS: ")
                    && parse_u32(v) == Some(WL_CONNECTED)
                {
                    joined = true;
                    break;
                }
            }
        }
        if joined {
            break;
        }
        writeln!(Console(&mut console), "  not joined yet, retrying in 5 s").ok();
        delay.delay_ms(5_000);
    }

    // Board IP, for the send command printed below (`AT+IPSTA=0` is the
    // read of WiFi.localIP(): 0 = IP_ADDR).
    let mut board_ip = [0u8; 16];
    let mut ip_len = 0usize;
    // The .ino waits out DHCP here: `while (WiFi.localIP() ==
    // IPAddress(0,0,0,0)) delay(200);` — the join reports connected
    // before the lease is in, and `AT+IPSTA=0` answers 0.0.0.0 until
    // then. Poll until the address is real; give up after ~10 s and
    // let the "board IP unknown" branch speak.
    for _ in 0..50 {
        drain(&mut delay, 20);
        if let Some((n, true)) = at(&mut delay, b"AT+IPSTA=0\r\n", &mut resp, 1_000)
            && let Some(v) = after(&resp[..n], b"+IPSTA: ")
            && !v.is_empty()
            && v.len() <= board_ip.len()
            && v.iter().any(|&b| b != b'0' && b != b'.')
        {
            board_ip[..v.len()].copy_from_slice(v);
            ip_len = v.len();
            break;
        }
        delay.delay_ms(200);
    }
    // The .ino prints the address as soon as it is real.
    if ip_len > 0 {
        let ip = core::str::from_utf8(&board_ip[..ip_len]).unwrap_or("?");
        writeln!(Console(&mut console), "Joined. Board IP: {}", ip).ok();
    }

    // From here on, every command is echoed, as the .ino's `atCommand`
    // does. Bridge version first: the echoed reply is the whole report.
    drain(&mut delay, 20);
    at_echo(
        &mut console,
        &mut delay,
        b"AT+FWVERSION?\r\n",
        &mut resp,
        2_000,
    );

    // Open the UDP socket: AT+UDPBEGIN=<port> -> +UDPBEGIN: <sock>\r\nOK\r\n
    let mut pn = [0u8; 10];
    let ps = u32_ascii(UDP_PORT, &mut pn);
    let cn = concat_into(
        &mut cmd,
        &[b"AT+UDPBEGIN=".as_slice(), ps, b"\r\n".as_slice()],
    );
    drain(&mut delay, 20);
    let (n, ok) = match at_echo(&mut console, &mut delay, &cmd[..cn], &mut resp, 2_000) {
        Some(r) => r,
        None => halt(&mut console, "AT+UDPBEGIN timed out"),
    };
    if !ok {
        halt(&mut console, "AT+UDPBEGIN failed");
    }
    let sock = match after(&resp[..n], b"+UDPBEGIN: ").and_then(parse_u32) {
        Some(s) => s,
        None => halt(&mut console, "no socket number in the +UDPBEGIN reply"),
    };

    // Push mode on: every datagram for this socket now arrives unasked.
    // A stock bridge answers ERROR here — that is how the .ino detects it.
    let mut sn = [0u8; 10];
    let ss = u32_ascii(sock, &mut sn);
    let cn = concat_into(
        &mut cmd,
        &[b"AT+UDPPUSH=".as_slice(), ss, b",1\r\n".as_slice()],
    );
    drain(&mut delay, 20);
    if !matches!(
        at_echo(&mut console, &mut delay, &cmd[..cn], &mut resp, 2_000),
        Some((_, true))
    ) {
        halt(
            &mut console,
            "AT+UDPPUSH was refused: this bridge has no push mode (stock firmware?)",
        );
    }

    writeln!(
        Console(&mut console),
        "\r\nPush mode is ON for socket {}.",
        sock
    )
    .ok();
    writeln!(Console(&mut console), "Send a datagram from the PC:").ok();
    if ip_len > 0 {
        // board_ip holds ASCII digits and dots only — valid UTF-8.
        let ip = core::str::from_utf8(&board_ip[..ip_len]).unwrap_or("?");
        writeln!(
            Console(&mut console),
            "  $bytes = [Text.Encoding]::ASCII.GetBytes(\"hello\"); (New-Object System.Net.Sockets.UdpClient).Send($bytes, $bytes.Length, '{}', {})",
            ip, UDP_PORT
        )
        .ok();
    } else {
        writeln!(
            Console(&mut console),
            "  (board IP unknown — AT+IPSTA=0 gave nothing; check your router)"
        )
        .ok();
    }
    writeln!(Console(&mut console), "Sleeping until a datagram arrives.").ok();

    let mut parser = FrameParser::new();
    let mut idle_wakeups: u32 = 0;

    loop {
        // Check for data and go to sleep with interrupts masked, so a
        // byte that arrives between the check and wfi() cannot be
        // missed: a pending interrupt still ends wfi(), and its
        // handler runs at enable() — the .ino's exact pattern.
        // UartIrq has no `available()`, so the check is a pop: the byte
        // it returns must go to the parser, not be dropped.
        cortex_m::interrupt::disable();
        let mut next = AT_LINK.read_byte();
        if next.is_none() {
            cortex_m::asm::wfi();
            // Safety: not inside a critical-section scope — re-enabling
            // interrupts here is exactly the .ino's `__enable_irq()`.
            unsafe { cortex_m::interrupt::enable() };
            next = AT_LINK.read_byte();
            if next.is_none() {
                idle_wakeups += 1;
                continue;
            }
        } else {
            // Safety: as above — this path leaves the masked region.
            unsafe { cortex_m::interrupt::enable() };
        }

        while let Some(b) = next {
            if parser.parse_byte(b).is_some() {
                report(&mut console, &parser, idle_wakeups);
                idle_wakeups = 0;
            }
            next = AT_LINK.read_byte();
        }
    }
}
