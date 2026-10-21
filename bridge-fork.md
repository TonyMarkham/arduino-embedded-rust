# Bridge fork — push-mode UDP for the UNO R4 WiFi

Implementation plan. Nothing in this file has been built, flashed or run
yet; every fact below is transcribed from source (listed per section), and
anything not verified is labeled **unverified**. There is no captured
output in this file — each step that produces output gets its real capture
added on the first run.

## Goal

The RA4M1 should sleep until a UDP datagram arrives, then act on it. The
stock ESP32-S3 bridge firmware cannot do that: it only speaks on the AT
link in answer to a command, so the RA4M1 has to poll (`AT+UDPPARSE`).
The fork adds one opt-in feature to the bridge — **push mode**: for a UDP
socket the RA4M1 has enabled, every received datagram is written down the
AT link unasked, as one `+UDPRX:` frame. The RA4M1's UART receive
interrupt then wakes it.

Everything else in the bridge stays as shipped: sketch uploads to the
RA4M1, USB serial pass-through (the SCI9 path), the 1200/2400-baud resets,
and every existing AT command. A sketch that never sends the new command
sees stock behavior.

## Where each piece lives

| Piece | Repo | License |
|---|---|---|
| Bridge fork (C++, Arduino-ESP32) | Your GitHub fork of `arduino/uno-r4-wifi-usb-bridge` | AGPL-3.0-or-later (inherited, see below) |
| Stage 1 example: stock bridge, interrupt-driven RA4M1, timer-paced polling | `unor4-hal`, new `publish = false` member | MIT |
| Stage 3 example: push-mode variant of Stage 1 | `unor4-hal`, same member | MIT — Rust only; refers to the fork by URL, contains none of its code |
| This plan | `arduino-embedded-rust` | — |

**Licensing.** The bridge's AT server (`chAT.hpp`, `parser.cpp`,
`server.cpp`, Copyright Reimu NotMoe / SudoMaker) carries the GNU Affero
General Public License v3-or-later header. Arduino's own bridge files
(`UNOR4USBBridge.ino`, `at_handler.*`, `cmds_*.h`) carry no license header,
and the repo has no top-level license file (GitHub's license API returns
404). A modified bridge is a derivative of the AGPL code, which is why it
stays out of the MIT `unor4-hal` repo. This is a reading of the license
headers, not legal advice.

## Prior work (surveyed 2026-10-21)

Surveyed: all 19 forks of `arduino/uno-r4-wifi-usb-bridge` (the 6 with
commits of their own compared against upstream), every issue and pull
request on that repo, GitHub repository search, and web search. Nobody
has published UDP push mode for the stock bridge. The closest work:

| Project | What it does | Relevance |
|---|---|---|
| [zmanu04/uno-r4-wifi-usb-bridge-ESP-NOW](https://github.com/zmanu04/uno-r4-wifi-usb-bridge-ESP-NOW) (2026-06, no license file) | Adds ESP-NOW AT commands; each received ESP-NOW message is pushed unasked as `+ESPNOWRECV:<mac>,<len>,<data>\r\n` | Same mechanism as this plan: the receive callback queues a task (`addTask`) that writes the frame through `at_srv.write_*`. Two things not to copy (reading of chAT's `server.cpp`, not tested): it writes its header from a local `char prefix[64]` with `write_cstr`, which queues a pointer that is sent after the buffer is gone; and it calls `addTask` from the ESP-NOW receive callback, while the task queue is a plain `std::queue` with no lock (ESP-IDF documents that callback as running in the Wi-Fi task, not the AT task — **unverified** against this core version). |
| [vshymanskyy/UNO-R4-WiFi-freedom](https://github.com/vshymanskyy/UNO-R4-WiFi-freedom) + [ESP_UNO_R4](https://github.com/vshymanskyy/ESP_UNO_R4) (2023-09, no license files) | PlatformIO library that repackages the bridge's USB serial pass-through, CMSIS-DAP, 1200-baud bootloader reset and HID `0xAA` reboot — without the AT server. You write your own ESP32 sketch. | A ready base for replacing the bridge outright (AGENTS.md option B). Built from a 2023 bridge; not updated since. |
| Bridge [issue #55](https://github.com/arduino/uno-r4-wifi-usb-bridge/issues/55) / [PR #56](https://github.com/arduino/uno-r4-wifi-usb-bridge/pull/56) | AT-link throughput | An Arduino developer reports 921600 baud working reliably in a custom build and higher rates not; a user measured ~4× throughput at 921600. The `vTaskDelay(1)` in the AT loop exists so the watchdog task can run — keep it. PR #56 (runtime baud change) notes that resetting the RA4M1 does not reset the ESP32, so a changed baud outlives the sketch that set it. |

The remaining active forks: Virus-V (a port to the BL616 chip),
andreagilardoni (Arduino's own feature branches; the fork's default branch
has nothing of its own), paulvha (ping and get-time, since merged
upstream), JAndrassy (no changes).

## What the stock bridge does (bridge tag `0.6.0`)

Sources: `arduino/uno-r4-wifi-usb-bridge` at tag `0.6.0` (the default
branch is two commits ahead: "Update Help Center article links" and its
merge), and the core it builds against,
`arduino/arduino-esp32` branch `unor4wifi-2.0.9`.

**AT server loop** — `UNOR4USBBridge.ino` runs `atLoop` as its own task
(`xTaskCreatePinnedToCore`); it calls `atHandler.run()` unless the USB
host has set 1200 or 2400 baud.
`CAtHandler::run()` (`at_handler.cpp`) is:

1. `at_srv.run()` — read bytes from the AT UART, parse, run command
   handlers, flush output;
2. run queued tasks (`addTask`) — used only by OTA, to keep calling
   `OTA.downloadPoll()` until a download finishes;
3. `vTaskDelay(1)`.

**Output queue** — command handlers do not write the UART directly. Every
`srv.write_*` call appends to a `std::deque` inside the chAT server
(`server.cpp`); `run()` flushes it after parsing. A handler's whole
response is appended inside one handler call, so anything else appended
later lands before or after a response, never inside one. The queue holds
at most 16384 bytes; past that, `write_raw` drops the **oldest** entries.

**Write helpers** (`server.cpp`):
- `write_response_prompt()` writes the name of the command *currently
  being parsed* plus `": "` — outside a handler it would repeat the last
  command's name, so push frames write their own prompt.
- `write_str(std::string)` and `write_vec8(std::vector<uint8_t>)` take
  their data by value (copied into the queue). `write_cstr` / `write_data`
  queue a raw pointer, so they are safe only with static strings.

**Unknown commands** — the command callback answers `ERROR` for any
command missing from `command_table`. A stock bridge therefore answers
`ERROR` to the new command, which is how the RA4M1 detects the fork.

**Unsolicited output** — `READY` once at boot (`SERIAL_AT.println("READY")`).
Nothing else.

**UDP sockets** — up to 4 (`MAX_UDP_AVAILABLE`), stored in
`CAtHandler::udps[]`. Created in `+UDPBEGIN` / `+UDPBEGINMULTI`
(`cmds_wifi_udp.h`), deleted in `+UDPSTOP` and in `+SOFTRESETWIFI`
(`cmds_esp_generic.h`, which deletes all of them).

**`WiFiUDP::parsePacket()`** (`libraries/WiFi/src/WiFiUdp.cpp` in the
patched core): one non-blocking `recvfrom` of up to 1460 bytes per call —
one datagram. It returns 0 while the previous datagram is still in its
receive buffer; `read()` frees that buffer once it is drained, and
`flush()` drops it. So datagrams come out one at a time, in order, only if
each one is read completely.

**Flash layout** (`tools/partitions/unor4wifi.csv` in the patched core;
`combine.py` uses the same offsets):

| Name | Offset | Size |
|---|---|---|
| bootloader | `0x0` | — |
| partition table | `0x8000` | — |
| otadata | `0x9000` | `0x2000` |
| cert | `0xB000` | `0x45000` |
| app0 | `0x50000` | `0x190000` |
| app1 | `0x1E0000` | `0x190000` |
| spiffs | `0x370000` | `0x80000` |
| nvs | `0x3F0000` | `0x10000` |

The bridge README's sample `esptool` line writes the app at `0x10000`;
that does not match this partition table. This plan uses `0x50000`.

**ESP32 download mode, two ways:**
- Software: the bridge's HID interface handles a feature report whose
  first byte is `0xAA` by calling `usb_persist_restart(RESTART_BOOTLOADER)`
  (`DAP.h`). Arduino's `unor4wifi-reboot` tool sends exactly that report
  to VID `0x2341` / PID `0x1002` (`unor4wifi-reboot/main.go`).
- Hardware: short the download pin to GND on the 3x2 header with the board
  unpowered, plug in USB, remove the jumper. The device then shows as
  **USB JTAG/serial debug unit** (Arduino tutorial
  [esp32-upload](https://docs.arduino.cc/tutorials/uno-r4-wifi/esp32-upload)).
  This is the fallback when the running bridge is broken.

**Arduino's restore kit** — release `0.6.0` asset
`unor4wifi-update-windows.zip` contains `update.bat`, `bin\unor4wifi-reboot.exe`,
`bin\espflash.exe` (a 2023 build) and `firmware\UNOR4-WIFI-S3-0.6.0.bin`
(1,966,080 bytes = `0x1E0000`, the same size as `combine.py`'s
`S3-BOOT-APP.bin` slice: bootloader through app0). `update.bat` runs
`unor4wifi-reboot`, waits 5 s, then
`espflash write-bin -b 115200 0x0 firmware\UNOR4-WIFI-S3-0.6.0.bin`.
It does not erase flash, so app1, spiffs and nvs are left alone.

**espflash flag change** — in espflash 4.6.0, `-b` is `--before` (the
reset mode) and the baud rate is `-B` (`espflash/src/cli/mod.rs`,
`ConnectArgs`). The `-b 115200` in `update.bat` and in the bridge README
belongs to the older bundled build. `write-bin` addresses must be
multiples of `0x1000`.

**Build** — `compile.sh` is one `arduino-cli compile` with the patched
core installed as a *user hardware* platform (`hardware/esp32-patched/`,
a submodule) and `directories.user` pointing at the repo. Its output lands
in `UNOR4USBBridge\build\esp32-patched.esp32.arduino_unor4wifi_usb_bridge\`
(the path `combine.py` reads and the `0.6.0` release zip contains).
`export.sh` / `combine.py` / `gen_crt_bundle.py` (Python) only build the
merged image; this plan does not need them — it writes Arduino's released
image first, then overlays only the app.

## The change

### New command

```
AT+UDPPUSH=<sock>,<0|1>\r\n   ->  +UDPPUSH: \r\nOK\r\n
```

- `1` enables push mode on UDP socket `<sock>`, `0` disables it.
- The `OK` / `ERROR` line is written by the chAT server from the
  handler's return value (`server.cpp`), as for every other command.
- `ERROR` for a bad argument or a socket that does not exist — and, on a
  stock bridge, for the command itself.
- Enabling also calls `flush()` on the socket, so a half-read datagram
  cannot block `parsePacket()`.

### Push frame

```
+UDPRX: <sock>,<len>|<len raw bytes>\r\n
```

Same size-prefixed shape as the existing `+UDPREAD` reply, so the RA4M1's
sized-read parser handles both. No `OK` follows — it is not a response.

### Rules the RA4M1 side must follow

- A `+UDPRX:` frame can arrive between any two responses — including
  after the RA4M1 has sent a command and before that command's reply.
  The receive parser handles it as its own frame.
- Never send `+UDPPARSE` / `+UDPREAD` for a socket in push mode; the
  bridge is already consuming its datagrams.
- Total traffic on the AT link must stay under its bandwidth (115200 baud
  ≈ 11.5 kB/s, frame headers included). If the 16384-byte output queue
  overflows, the bridge drops its oldest queued entries, which can cut a
  frame. The parser resynchronizes by discarding up to the next line
  start (`\r\n`) followed by `+`. That is a heuristic: a payload that
  itself contains `\r\n+` can mislead it.
- **Push mode survives an RA4M1 reset.** Resetting the RA4M1 does not
  reset the ESP32 (PR #56's observation), so sockets and their push flags
  outlive the sketch. A freshly booted RA4M1 can receive `+UDPRX:` frames
  before it has sent anything. Boot must drain the line, then send
  `AT+SOFTRESETWIFI` (which deletes every UDP socket, and with it every
  push flag) before it relies on any reply. wifi-sample's handshake
  drains and soft-resets, but it treats "any reply" to `AT` as alive —
  a stray frame must not count as that reply.
- **Frame size.** A datagram is up to 1460 bytes (`parsePacket`'s
  buffer), plus a header of about 20 bytes. `UartIrq`'s receive ring
  defaults to 256 bytes; it only overflows if the main loop stops
  draining for longer than 256 byte times (about 22 ms at 115200). The
  parser's own payload buffer is the real limit: it either holds the
  largest datagram the RPi sends, or discards oversize frames by their
  `<len>` field without losing sync.

### Code sketches (C++, untested)

Each sketch below uses only calls that already appear in the bridge
source. They are design sketches for the owner to adapt, not tested code.

`commands.h` — next to the other UDP command names:

```cpp
#define _UDPPUSH                 "+UDPPUSH"
```

`at_handler.h` — in `CAtHandler`'s private members, next to `udps`:

```cpp
   bool udp_push[MAX_UDP_AVAILABLE];
```

`at_handler.cpp` — in the constructor's existing `udps[i] = nullptr;`
loop:

```cpp
    udp_push[i] = false;
```

Every place a `udps[...]` slot is created or deleted also sets that
slot's `udp_push[...] = false`: `+UDPBEGIN`, `+UDPBEGINMULTI` and
`+UDPSTOP` in `cmds_wifi_udp.h`, and the delete-all loop in
`+SOFTRESETWIFI` (`cmds_esp_generic.h`). A new socket therefore always
starts in stock (polled) mode.

`cmds_wifi_udp.h` — a new entry inside `add_cmds_wifi_udp()`:

```cpp
   command_table[_UDPPUSH] = [this](auto & srv, auto & parser) {
      switch (parser.cmd_mode) {
         case chAT::CommandMode::Write: {
            if (parser.args.size() != 2) {
               return chAT::CommandStatus::ERROR;
            }
            auto &sock_num = parser.args[0];
            auto &enable = parser.args[1];
            if (sock_num.empty() || enable.empty()) {
               return chAT::CommandStatus::ERROR;
            }
            int sock = atoi(sock_num.c_str());
            if (sock < 0 || sock >= MAX_UDP_AVAILABLE || udps[sock] == nullptr) {
               return chAT::CommandStatus::ERROR;
            }
            udps[sock]->flush();
            udp_push[sock] = atoi(enable.c_str()) != 0;
            srv.write_response_prompt();
            srv.write_line_end();
            return chAT::CommandStatus::OK;
         }
         default:
            return chAT::CommandStatus::ERROR;
      }
   };
```

`at_handler.cpp` — in `CAtHandler::run()`, between `at_srv.run();` and
the task loop:

```cpp
  // Push mode: forward one whole datagram per enabled socket per pass.
  for (int i = 0; i < MAX_UDP_AVAILABLE; i++) {
    if (udps[i] == nullptr || !udp_push[i]) {
      continue;
    }
    int len = udps[i]->parsePacket();
    if (len <= 0) {
      continue;
    }
    std::vector<uint8_t> data(len);
    int got = udps[i]->read(data.data(), len);
    data.resize(got > 0 ? got : 0);
    String header = "+UDPRX: " + String(i) + "," + String(data.size()) + "|";
    at_srv.write_str(header.c_str());   // copied: header is a temporary
    at_srv.write_vec8(data);
    at_srv.write_line_end();
  }
```

The frame is queued after this pass's flush, so it goes out on the next
`at_srv.run()` — one loop iteration later. **Unverified:** the duration of
`vTaskDelay(1)` (one FreeRTOS tick; the tick rate of this core build has
not been checked).

The hook runs in the AT task itself, so it needs no lock: `udps[]`, the
task queue and `at_srv` are only touched from that task. A receive
callback (AsyncUDP, or the ESP-NOW fork's pattern) would run in another
task and would need one.

### Optional: a faster AT link

Both ends must use the same baud, set at compile time on each side:

- **Bridge:** `UNOR4USBBridge.ino` line 160,
  `SERIAL_AT.begin(115200, SERIAL_8N1, 6, 5);` — the line the issue #55
  reporter changed. Line 139 has an earlier `SERIAL_AT.begin(115200);`
  that precedes `READY`. **Unverified:** whether it must change too.
- **RA4M1:** the `baud` argument of `Uart::sci1` / `UartIrq::sci1`
  (`115_200` in wifi-sample).

`unor4-hal`'s `calc_baud` rejects any rate it cannot reach within 3% of
PCLKA (48 MHz), and it does not use bit-rate modulation. Running its
algorithm for the candidates gives:

| Baud | RA4M1 error | Notes |
|---|---|---|
| 115200 | 0.16% | stock |
| 230400 | 0.16% | |
| 460800 | 0.16% | in the ESP32 list from issue #55 |
| 921600 | **rejected** (nearest divisor is 8.5% off) | the rate issue #55 found reliable — out of reach for `unor4-hal` without modulation; `Uart::sci1` panics |
| 1000000 | 0% | the Arduino developer reported unreliable results above 921600 (his test setup is not stated) |

These numbers were computed with a Python transcription of `calc_baud`,
not with the crate itself. **Unverified:** stable operation at 460800 or
1000000 on this board; settle it with a host unit test for the numbers
and a hardware soak test for the link.

Keep the baud a compile-time constant on both sides. A runtime change
(PR #56's approach) leaves the bridge at the new rate after an RA4M1
reset, and the next sketch cannot reach it.

## Where the work is

The bridge diff is small — one command, one flag array, one hook, a few
`false` assignments. Almost all of the effort is elsewhere:

| Area | What makes it more than the diff |
|---|---|
| Building the bridge on Windows (Part 1) | Never run in this course. The scripts are bash; the toolchain comes from `get.exe`; the patched ESP32 core is a submodule pinned to a 2.0.9-era branch. Any of these can fail on first contact. |
| Flashing and recovery (Parts 2, 6) | A new skill set (download mode, espflash, partition offsets), with soft-brick risk on every write and three unverified steps (device name, reset after write, `otadata`). |
| Proving the fork changed anything (Part 4) | Parts 2 and 4 look identical from outside until the RA4M1 can send `AT+UDPPUSH` — the feature check needs Part 5 first. |
| The RA4M1 receive path (Part 5) | The biggest piece. A parser that accepts unsolicited frames between any two responses, sized reads, oversize-frame discard, resync after loss, boot-time cleanup of push state left by an earlier sketch, the sleep loop, and the failsafe timer. None of it exists yet — the Stage 1 example has to be written first. |
| Testing the edges | Datagrams faster than the link, datagrams larger than the parser buffer, RA4M1 reset while the RPi keeps sending, bridge queue overflow. Each needs a deliberate test from the PC or RPi side. |
| Faster AT link (optional) | Changes both firmwares at once and needs a soak test (see "Optional: a faster AT link"). |

## Part 1 — Build the stock bridge, unchanged

Proves the toolchain before any change. The bridge's own scripts are bash;
the commands below are their PowerShell equivalents. **Unverified:** this
build has not been run on Windows in this course.

Prerequisites: `git`, `arduino-cli` (installed in Part 0 of the course).

> **Step 1.1 — Fork the repository**
>
> On GitHub, fork `arduino/uno-r4-wifi-usb-bridge` to your account. The
> fork's default branch is `main` (= tag `0.6.0` plus a Help Center link
> update and its merge commit).

> **Step 1.2 — Clone your fork**
>
> Clone it next to your other repos (replace `<you>` with your GitHub
> user name):
>
> ```powershell
> git clone https://github.com/<you>/uno-r4-wifi-usb-bridge.git D:\git\uno-r4-wifi-usb-bridge
> ```

> **Step 1.3 — Fetch the submodules**
>
> The patched ESP32 core, BOSSA and the libraries are submodules. This is
> the bridge README's own command, run from the clone:
>
> ```powershell
> git -C D:\git\uno-r4-wifi-usb-bridge submodule update --init --depth 1 --no-single-branch
> ```
>
> Verify the patched core arrived — this lists its `tools` folder, which
> must contain `get.exe`:
>
> ```powershell
> Get-ChildItem D:\git\uno-r4-wifi-usb-bridge\hardware\esp32-patched\esp32\tools
> ```

> **Step 1.4 — Download the ESP32 toolchain**
>
> The README runs `./get.py`; the patched core also ships a Windows build
> of the same script, `get.exe`, which needs no Python. **Unverified:**
> that `get.exe` behaves identically. Run it from its own folder:
>
> ```powershell
> Set-Location D:\git\uno-r4-wifi-usb-bridge\hardware\esp32-patched\esp32\tools
> ```
>
> ```powershell
> .\get.exe
> ```

> **Step 1.5 — Generate `arduino-cli.yaml`**
>
> `compile.sh` copies `arduino-cli.yaml.orig` and replaces the word `PWD`
> with the repo path, so arduino-cli finds `hardware\esp32-patched` as a
> user platform. Go back to the repo root:
>
> ```powershell
> Set-Location D:\git\uno-r4-wifi-usb-bridge
> ```
>
> Write the config with forward slashes in the path (**unverified:**
> whether backslashes would also work in this YAML field):
>
> ```powershell
> (Get-Content arduino-cli.yaml.orig) -replace 'PWD', ($PWD.Path -replace '\\', '/') | Set-Content arduino-cli.yaml
> ```
>
> Verify the `user:` line now holds the repo path:
>
> ```powershell
> Select-String -Path arduino-cli.yaml -Pattern 'user:'
> ```

> **Step 1.6 — Compile**
>
> This is `compile.sh`'s command. The FQBN is quoted because PowerShell
> would otherwise split its commas into separate arguments:
>
> ```powershell
> arduino-cli compile -e --config-file arduino-cli.yaml --fqbn "esp32-patched:esp32:arduino_unor4wifi_usb_bridge:JTAGAdapter=default,PSRAM=disabled,FlashMode=qio,FlashSize=4M,LoopCore=1,EventsCore=1,USBMode=default,CDCOnBoot=default,MSCOnBoot=default,DFUOnBoot=default,UploadMode=default,PartitionScheme=unor4wifi,CPUFreq=240,UploadSpeed=921600,DebugLevel=none,EraseFlash=none" -v UNOR4USBBridge
> ```

> **Step 1.7 — Find the app image**
>
> The app image is the only file this plan flashes from your build:
>
> ```powershell
> Get-Item UNOR4USBBridge\build\esp32-patched.esp32.arduino_unor4wifi_usb_bridge\UNOR4USBBridge.ino.bin
> ```
>
> For scale: Arduino's own `0.6.0` build of this file is 1,180,064 bytes;
> it must stay under app0's `0x190000` (1,638,400) bytes.

✅ **Checkpoint:** `UNOR4USBBridge.ino.bin` exists and is smaller than
1,638,400 bytes.

## Part 2 — Prove the flash path with the unchanged build

Flashes Arduino's released image, then your unchanged app on top of it,
and checks the board still uploads and joins WiFi. If this part fails, the
problem is the toolchain or the flash procedure, not the fork.

> [!IMPORTANT]
> Before writing anything to the ESP32-S3, have the restore kit
> (Step 2.1) unpacked. If the bridge stops working, uploads to the RA4M1
> stop too; Part 6 restores it. If even the software route into download
> mode fails, use the hardware jumper (download pin to GND, board
> unpowered) from Arduino's
> [esp32-upload](https://docs.arduino.cc/tutorials/uno-r4-wifi/esp32-upload)
> tutorial.

> [!IMPORTANT]
> **Close the Arduino IDE before any `espflash` write.** Arduino's
> `serial-discovery` 1.5.0 actively probes USB devices. While it runs,
> `espflash write-bin` to this board can fail with `Bad data checksum`
> and leave the bridge soft-bricked
> ([bridge issue #78](https://github.com/arduino/uno-r4-wifi-usb-bridge/issues/78),
> [serial-discovery issue #128](https://github.com/arduino/serial-discovery/issues/128)).
> The Arduino IDE keeps `serial-discovery` running in the background.
> `serial-discovery` 1.5.1 (2026-07-15) probes only devices with
> Arduino's USB vendor ID, which fixed the reported fault; 1.4.3 is
> reported unaffected. **Unverified:** that `arduino-cli board list`
> stops `serial-discovery` when it exits. If the error appears, recover with
> the jumper route (Step 6.2) once nothing else is using the port.

> **Step 2.1 — Get Arduino's restore kit**
>
> Download `unor4wifi-update-windows.zip` from the
> [0.6.0 release](https://github.com/arduino/uno-r4-wifi-usb-bridge/releases/tag/0.6.0)
> and unpack it to `D:\git\unor4wifi-update-windows`. It holds the stock
> image and `unor4wifi-reboot.exe`. Verify the image size is 1,966,080
> bytes:
>
> ```powershell
> (Get-Item D:\git\unor4wifi-update-windows\firmware\UNOR4-WIFI-S3-0.6.0.bin).Length
> ```

> **Step 2.2 — Install espflash 4.6.0**
>
> espflash is a Rust tool (esp-rs), installed with cargo:
>
> ```powershell
> cargo install espflash --version 4.6.0 --locked
> ```
>
> Verify:
>
> ```powershell
> espflash --version
> ```

> **Step 2.3 — Put the ESP32-S3 into download mode**
>
> `unor4wifi-reboot.exe` sends the bridge the HID report that restarts the
> ESP32-S3 into its ROM bootloader. With the board plugged in:
>
> ```powershell
> D:\git\unor4wifi-update-windows\bin\unor4wifi-reboot.exe
> ```
>
> List the serial ports. The board re-enumerates as the ESP32-S3 itself,
> not the UNO R4 WiFi, and the COM number may change. **Unverified:** the
> device name on this software route — Arduino's tutorial documents
> **USB JTAG/serial debug unit** only for the jumper route.
>
> ```powershell
> espflash list-ports
> ```
>
> Capture it (use the port from the listing):
>
> ```powershell
> $espPort = 'COM7'   # <- the ESP32-S3's port from the listing above
> ```

> **Step 2.4 — Write Arduino's stock image**
>
> This writes bootloader, partition table, otadata, certificates and the
> stock app0 — the same write `update.bat` does, with espflash 4.6.0's
> flags. It also rewrites `otadata` (`0x9000`). **Unverified:** that the
> rewritten `otadata` selects app0 — inferred from the layout and the
> `boot_app0.bin` name `combine.py` places there; Part 4's probe confirms
> which app runs.
>
> ```powershell
> espflash write-bin -p $espPort 0x0 D:\git\unor4wifi-update-windows\firmware\UNOR4-WIFI-S3-0.6.0.bin
> ```
>
> espflash's default `--after` is `hard-reset`. **Unverified:** whether
> that restarts the bridge cleanly over USB-JTAG; if the UNO R4 WiFi port
> does not come back, unplug and replug the board.

> **Step 2.5 — Overlay your own app build**
>
> Re-enter download mode:
>
> ```powershell
> D:\git\unor4wifi-update-windows\bin\unor4wifi-reboot.exe
> ```
>
> ```powershell
> espflash list-ports
> ```
>
> ```powershell
> $espPort = 'COM7'   # <- re-set if the number changed
> ```
>
> Write your Part 1 build to app0 at `0x50000`, leaving everything else
> as Step 2.4 wrote it:
>
> ```powershell
> espflash write-bin -p $espPort 0x50000 D:\git\uno-r4-wifi-usb-bridge\UNOR4USBBridge\build\esp32-patched.esp32.arduino_unor4wifi_usb_bridge\UNOR4USBBridge.ino.bin
> ```

> **Step 2.6 — Regression check: the board is an UNO R4 WiFi again**
>
> ```powershell
> arduino-cli board list
> ```
>
> The board's line must show FQBN `arduino:renesas_uno:unor4wifi`.

> **Step 2.7 — Regression check: upload and join with `wifi-sample`**
>
> `wifi-sample` exercises both bridge paths: the upload (SAM-BA through the
> bridge) and the AT link (WiFi join). Build it from
> `crates\wifi-sample` (its `.env` must be filled in):
>
> ```powershell
> Set-Location D:\git\arduino-embedded-rust\crates\wifi-sample
> ```
>
> ```powershell
> cargo build --release
> ```
>
> ```powershell
> cargo objcopy --release -- -O binary firmware.bin
> ```
>
> ⚠ **Never upload cold** — double-tap reset (LED fades = bootloader
> mode), then confirm the port and set `$port`:
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
>
> The D13 LED goes solid once the board has joined (wifi-sample's
> README has the full LED table).

✅ **Checkpoint:** with your self-built bridge, the board lists as an UNO
R4 WiFi, accepts an upload, and `wifi-sample` reaches solid LED. Part 2
cannot tell your build from Arduino's — both are the same source. Part 4's
`AT+UDPPUSH` probe is the first check that tells them apart.

## Part 3 — Implement push mode

> **Step 3.1 — Make the changes from "The change"**
>
> In `D:\git\uno-r4-wifi-usb-bridge\UNOR4USBBridge`:
> - `commands.h`: the `_UDPPUSH` define;
> - `at_handler.h`: the `udp_push` member;
> - `at_handler.cpp`: the constructor initialization and the `run()` hook;
> - `cmds_wifi_udp.h`: the `+UDPPUSH` handler, and `udp_push[i] = false`
>   in `+UDPBEGIN`, `+UDPBEGINMULTI`, `+UDPSTOP`;
> - `cmds_esp_generic.h`: `udp_push[i] = false` in `+SOFTRESETWIFI`'s
>   delete-all loop.
>
> Read each file on disk before editing; the sketches above show the
> intent, not the surrounding code.

> **Step 3.2 — Rebuild**
>
> From the repo root:
>
> ```powershell
> Set-Location D:\git\uno-r4-wifi-usb-bridge
> ```
>
> ```powershell
> arduino-cli compile -e --config-file arduino-cli.yaml --fqbn "esp32-patched:esp32:arduino_unor4wifi_usb_bridge:JTAGAdapter=default,PSRAM=disabled,FlashMode=qio,FlashSize=4M,LoopCore=1,EventsCore=1,USBMode=default,CDCOnBoot=default,MSCOnBoot=default,DFUOnBoot=default,UploadMode=default,PartitionScheme=unor4wifi,CPUFreq=240,UploadSpeed=921600,DebugLevel=none,EraseFlash=none" -v UNOR4USBBridge
> ```

✅ **Checkpoint:** the fork compiles, and `UNOR4USBBridge.ino.bin` is
still smaller than 1,638,400 bytes.

## Part 4 — Flash the fork and verify

> [!IMPORTANT]
> Close the Arduino IDE first — `serial-discovery` 1.5.0 can corrupt
> `espflash` writes (Part 2's callout has the details).

> **Step 4.1 — Put the ESP32-S3 into download mode**
>
> ```powershell
> D:\git\unor4wifi-update-windows\bin\unor4wifi-reboot.exe
> ```
>
> ```powershell
> espflash list-ports
> ```
>
> ```powershell
> $espPort = 'COM7'   # <- the ESP32-S3's port from the listing above
> ```

> **Step 4.2 — Write the fork's app to app0**
>
> Only the app changes; Step 2.4's bootloader, partitions and certificates
> stay:
>
> ```powershell
> espflash write-bin -p $espPort 0x50000 D:\git\uno-r4-wifi-usb-bridge\UNOR4USBBridge\build\esp32-patched.esp32.arduino_unor4wifi_usb_bridge\UNOR4USBBridge.ino.bin
> ```

> **Step 4.3 — Regression check**
>
> Repeat Steps 2.6 and 2.7 (board list, `wifi-sample` upload, solid
> LED). The fork must pass every stock check before the new feature is
> tested.

> **Step 4.4 — Feature check**
>
> Needs the RA4M1 push-mode firmware from Part 5:
> 1. `AT+UDPBEGIN=<port>` → `+UDPBEGIN: <sock>`;
> 2. `AT+UDPPUSH=<sock>,1` → `OK` (a stock bridge answers `ERROR` here);
> 3. send a datagram from the PC or RPi to the board's IP and port;
> 4. the RA4M1 receives `+UDPRX: <sock>,<len>|<bytes>\r\n` without sending
>    any command, and echoes the payload on SCI9 (`arduino-cli monitor`).

✅ **Checkpoint:** stock behavior intact, and a datagram reaches the RA4M1
with no poll from its side.

## Part 5 — RA4M1 side (the Stage 3 example in `unor4-hal`)

Outline only; the Stage 1 example (stock bridge, timer-paced polling)
comes first and provides the AT client, the sized-read parser and the
`key=value` handling this builds on.

- **Startup** — join WiFi (wifi-sample's sequence), `AT+UDPBEGIN=<port>`,
  then `AT+UDPPUSH=<sock>,1`.
  - `OK` → push mode: no SysTick-driven polling.
  - `ERROR` → stock bridge: fall back to Stage 1's timer-paced polling.
    One image runs on either bridge.
- **Boot cleanup** — before the handshake counts as done: drain, then
  `AT+SOFTRESETWIFI`, so push flags left by an earlier sketch are gone
  (see "Rules the RA4M1 side must follow").
- **Receive** — `UartIrq<SCI1>`; the main loop checks the receive ring
  with interrupts masked, calls `wfi()` when it is empty, and on wake
  feeds bytes to a parser that recognizes `+UDPRX:` frames as well as
  command responses, and discards oversize frames by their `<len>`.
- **Act** — parse `key=value`, set the PWM duty.
- **Failsafe** — still needs a timer: no valid frame within N ms → stop
  the motors. This is the only periodic wake in push mode.
- **Unverified:** whether `wfi()` enters Sleep mode (SCI keeps receiving)
  or Software Standby on the RA4M1 — `SBYCR.SSBY` in the hardware manual.

## Part 6 — Restore the stock bridge

Arduino's kit writes the stock image from `0x0` (bootloader through app0),
which replaces the fork's app.

> [!IMPORTANT]
> Close the Arduino IDE first — `serial-discovery` 1.5.0 can corrupt
> `espflash` writes (Part 2's callout has the details).

> **Step 6.1 — Run Arduino's updater**
>
> `update.bat` enters download mode with `unor4wifi-reboot`, then writes
> the stock image with its bundled espflash:
>
> ```powershell
> D:\git\unor4wifi-update-windows\update.bat
> ```

If `unor4wifi-reboot` cannot reach the bridge (a broken fork), use the
hardware jumper to enter download mode, then write the stock image with
espflash 4.6.0:

> **Step 6.2 — Restore by hand (jumper route)**
>
> With the board in download mode (jumper method), find its port:
>
> ```powershell
> espflash list-ports
> ```
>
> ```powershell
> $espPort = 'COM7'   # <- the ESP32-S3's port from the listing above
> ```
>
> ```powershell
> espflash write-bin -p $espPort 0x0 D:\git\unor4wifi-update-windows\firmware\UNOR4-WIFI-S3-0.6.0.bin
> ```

✅ **Checkpoint:** `arduino-cli board list` shows the UNO R4 WiFi, and
`wifi-sample` uploads and joins.

## Open items

| Item | Status | How to settle it |
|---|---|---|
| Windows build of the bridge (Part 1) | unverified | Run Part 1 |
| `get.exe` equivalent to `get.py` | unverified | Step 1.4 succeeds and Step 1.6 finds the compiler |
| Device name after `unor4wifi-reboot` (software download mode) | unverified | Observe Step 2.3 |
| Board comes back after `espflash` hard-reset over USB-JTAG | unverified | Observe Step 2.4 |
| Stock image's `otadata` selects app0 | unverified (inferred) | Part 4's `AT+UDPPUSH` probe answers `OK` |
| `vTaskDelay(1)` duration (push latency floor) | unverified | Check `CONFIG_FREERTOS_HZ` in the patched core's sdkconfig for ESP32-S3 |
| `wfi()` sleep mode on the RA4M1 | unverified | RA4M1 hardware manual, `SBYCR.SSBY` |
| Faster AT link (460800 or 1000000) | unverified | Host unit test of `calc_baud` for the rate, then a hardware soak test |
| Whether bridge line 139's `SERIAL_AT.begin(115200)` must change with the baud | unverified | Read `UNOR4USBBridge.ino` setup order; observe `READY` at the new rate |
| Publishing terms of the fork (AGPL files + unlicensed Arduino files) | open | Read the AGPL obligations before making the fork public |
