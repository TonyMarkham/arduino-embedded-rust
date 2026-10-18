# AGENTS.md — rules for AI agents working in this repo

This is a **learning project**. The owner is learning embedded Rust by doing
every step themselves. These rules exist because agents violated them before.

## Project goal

POC firmware: the UNO R4 WiFi receives a **data packet over WiFi** and displays
a **pattern on its LED matrix**. The repo is a cargo workspace so a protocol
crate can be shared between the embedded firmware and host-side tooling.
Target architecture:

- `crates/arduino-embedded-rust` — firmware (RA4M1, `thumbv7em-none-eabihf`)
- `crates/protocol` — packet format, `no_std`-compatible, shared with host
- `crates/host-tool` (optional) — PC sender for testing

Known-hard parts (research before designing, source = Arduino's C++ libs):
- WiFi: RA4M1 has no radio; it drives the ESP32-S3 bridge over UART. Rust must
  implement the bridge protocol subset (see `WiFiS3` / `uno-r4-wifi-usb-bridge`
  source).
- LED matrix: ownership (RA4M1 vs ESP32-S3) must be verified from the
  `Arduino_LED_Matrix` library and board schematic before writing a driver.

## Non-negotiable

1. **Never write project code or config for the owner.** No creating/editing
   `Cargo.toml`, `src/*`, `memory.x`, `build.rs`, `.cargo/*`, scripts, etc.
   The only files an agent may write are documentation (README.md, AGENTS.md).
   If a fix requires code changes, give the owner the file contents + explanation
   in chat and let them apply it.
2. **Never run build/flash/install commands on the owner's behalf.** Guide,
   don't execute. Checkpoints in README.md are the owner's to pass.
3. **No editorializing in responses or docs.** No "lesson value" asides, no
   moralizing, no praise, no platitudes, no "good instinct!" commentary.
   State facts and fixes. Stop there.
4. **If you don't know, research. Guessing is the death of learning.** This
   repo's owner trusts what you write and builds on it — one wrong fact (a
   package ID, a crate API, a pin mapping, a memory address) sends them hours
   down a dead end. Therefore:
   - Never state anything from memory that you cannot verify. Commands, package
     IDs, versions, crate APIs, register names, URLs — search the web, read
     docs, or inspect source first.
   - The owner is watching real output (e.g. they caught a wrong winget ID with
     `winget search`). Prefer giving them a one-line *verification command* over
     asserting an unverified value.
   - "I don't know — here's how to find out" is always an acceptable answer.
     A confident guess is never acceptable.
   - If something is unverified, label it as such explicitly.
5. **Always read the file on disk before instructing the owner to edit it.**
   Never give edit instructions based on what a file "should" contain or what
   an earlier step created — read the current bytes first (imports, layout,
   formatting, drift from the docs). Instructions issued blind risk having
   the owner overwrite their own work.

## Board-safety constraints (Arduino UNO R4 WiFi)

- The Arduino bootloader occupies flash `0x00000000–0x00003FFF`. Never suggest
  anything that writes below `0x4000` (memory.x FLASH ORIGIN must stay
  `0x00004000, LENGTH = 240K`).
- Never suggest Renesas `rfp-cli`, raw `bossac` erase/offset commands, or
  full-chip images as a way to "clear" the board. Safe wipe = upload any valid
  sketch with `arduino-cli upload` (bootloader untouched).
- The HAL dependency must stay
  `arduino-uno-r4-hal`, default-features off, feature `uno-r4-wifi` (the Minima
  pin mapping would silently target the wrong pins).

## Documentation conventions (README.md, Basics.md, BlinkSketch.md, BlinkEmbedded.md)

- **The documentation set is the single source of truth for the learning
  path.** README.md is the summary; Basics.md / BlinkSketch.md /
  BlinkEmbedded.md carry the steps. Chat messages must point the owner at doc
  steps (e.g. "BlinkEmbedded Step 5.3"), not issue parallel instructions. If
  chat ever disagrees with the docs, the docs win — and the agent fixes the
  docs immediately if they are the wrong one.
- Every actionable step is a blockquote (`>` block), numbered per part
  (`Step 2.3`). Concept explanations sit between blockquotes as plain text.
- **One purpose per code block.** Never stack multiple commands into one
  block "to save space". Each block does one thing, gets a one-line
  explanation, and where sensible a way to verify it worked. A failure must
  point at one specific block. Multi-command sequences become sub-steps
  (e.g. Step 1.3a/1.3b/1.3c).
- **Explain before showing.** Every command block is preceded by a
  description of what it does and why — never "code first, explanation
  after". (Exception: file-creation steps show the file contents with inline
  comments; the deeper what/why follows, since the file itself is the thing
  being typed.)
- **No bare cross-references in place of commands.** Never write "recompile
  (Step 1.3b)" where the owner needs the command. Every step shows the full
  command(s) it requires — repetition across steps is fine; copy-paste must
  work at the point of use.
- **Rewrite, don't accrete.** When a section accumulates multiple ⚠/✅
  patches, duplicated command blocks, or a failure ladder bolted onto a step,
  rewrite the section cleanly. The doc should read like it was written once,
  by someone who already knows the pitfalls.
- **Re-read after every edit.** After changing any step, re-read its whole
  section — heading through checkpoint, plus the steps before and after —
  and fix contradictions immediately. An edit that leaves its neighbors
  stale is an incomplete edit.
- **Every upload command is preceded by the "Never upload cold" admonition**
  (double-tap reset → LED fades = bootloader mode → re-check port with
  `board list` → then upload). Bootloader mode avoids the port
  re-enumeration race that kills uploads mid-write.
- **Survival-critical reference content is a blockquote callout, not prose.**
  The owner scans this doc by blockquote — anything that must not be missed
  (reset button behavior, flashing rules) is formatted as a blockquote
  (GitHub `> [!IMPORTANT]` where appropriate), never as plain paragraphs.
- **No git or repo chores in the docs.** No `.gitignore` dumps, no `git
  add`/`commit` steps, no VCS workflow. The docs teach embedded Rust; the
  owner manages version control themselves.
- **When the owner's files exist, doc file-content blocks mirror them
  exactly** — contents *and* formatting (alignment, pins, spacing). The doc
  never "improves" the owner's style.
- **Every command with checkable output gets an "Expected output" block**
  containing real captured output from this course's runs (never invented
  placeholder text). The owner compares their screen against it.
- Each part ends with a ✅ **Checkpoint** line.
- PowerShell is the shell — all commands must run in PowerShell 7 on Windows.
