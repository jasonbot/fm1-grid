# FM-1 Grid

Up to four emulated M-VAVE FM-1s side by side, each with its own firmware and LCD, with a
Platinum-style (Mac OS 9) light interface. Two run by default; `--slots 4` starts all four
on a machine with enough cores. Click anywhere in a slot to route the computer keyboard
there; the selected card gets a blue border and the keyboard indicator in its title bar.
The large, clickable FM-1 front panel above the metronome mirrors the active slot: its keys
and buttons can be played with the mouse, its knobs turn with the −/+ ends, and its LEDs
show the firmware's own state as well as what the computer keyboard is doing.

```
┌─ SLOT 1 ─────────┐ ┌─ SLOT 2 ─────────┐
│ ┌──────────────┐ │ │ ┌──────────────┐ │
│ │   240×240    │ │ │ │   240×240    │ │
│ └──────────────┘ │ │ └──────────────┘ │
│ SLOOP 2.8        │ │ Felucca 1.6.1    │
│ [ firmware ▾ ]   │ │ [ firmware ▾ ]   │
└──────────────────┘ └──────────────────┘
┌─ FM-1 PANEL ────────────────────────── CLICKABLE · KEYBOARD LIGHTS · SLOT 1 ──┐
│ [− MASTER +]  [− SELECT +]                 [− KNOB 1 +] … [− KNOB 4 +]       │
│ [− PRESETS +] [− ALGORITHM +]                                               │
│ [OCT− Z] [OCT+ X]   [FX 1][SCL 2][ENV 3][LFO 4][EDIT 5][GLO 6]              │
│                     [HOME 7][SAVE 8][ARP 9][SEQ 0][PLAY ␣][REC R]            │
│ [F3][F#3][G3] … [C4 A][C#4 W][D4 S] … [F5 '][F#5][G5]                       │
└──────────────────────────────────────────────────────────────────────────────┘
[ ○─── ] MIDI CLOCK  120 BPM [ − ][ + ]  ● BEAT
         STOPPED · 24 PPQN
```

## Run

```sh
cargo run --release
```

With no arguments the first two slots start with an embedded firmware:

| Slot | Firmware |
|------|----------|
| 1 | SLOOP 2.8 |
| 2 | Felucca 1.6.1 |

Either slot can load any of the four embedded firmwares (including FM1 Open V15 and
ChoralRoot 0.26.1) from its dropdown.

Choose the number of slots with `--slots N` (1–4, default 2). With no firmware arguments,
the first N embedded firmwares start. Four is practical on a 4-core machine; more than four
would oversubscribe the physical cores.

```sh
cargo run --release -- --slots 4
```

Pass up to N firmware files to choose what starts where:

```sh
cargo run --release -- --slots 4 sloop.fwsc felucca.fwsc
```

A `.fwsc` file is treated as an update package; a `.bin` or `.xip` file is treated as a raw
app image mapped at XIP base, entered at `0x0200_0120`. `--help` lists the options.

### Firmware dropdown

Every slot's dropdown offers the four embedded firmwares and **Other…**, which opens a file
picker. The embedded images are compiled into the binary (see `firmware/README.md` for
versions, sources and SHA-256 hashes). Hovering a choice shows its file name and hash.
Loading a firmware stops the slot's old emulator, clears its audio queue and starts the new
one; the metronome is reconnected automatically.

## Keyboard

The active slot receives the same layout as `fm1-live`:

| Keys | FM-1 |
|------|------|
| `A W S E D F T G Y H U J K O L P ; '` | C4 … F5 (key IDs 21–38) |
| `Z` / `X` | octave down / up (IDs 0, 1) |
| `1 2 3 4 5 6 7 8 9 0` | function buttons (IDs 2–11) |
| `Space` | PLAY / PAUSE (ID 12) |
| `R` | REC (ID 13) |
| `←` / `→` | SELECT encoder |
| `↓` / `↑` | ALGORITHM encoder |
| `,` / `.` | PRESETS encoder |

Keys are sent as key-down/key-up events to the active emulator. Switching slots releases
held keys on the old one, so no note sticks. Clicking anywhere in a slot's box makes it
active; the blue outline and the `KEYBOARD` label in its title bar show which slot has the
keyboard.

The FM-1 panel is also a control surface. The 27 white and black note keys are tall and can
be clicked and held. MASTER, SELECT, PRESETS and ALGORITHM sit in a block at the upper left;
KNOB 1–4 are to their right. Each of those controls is a button group with a small −/+ end
on the left and right that turns it one detent. Each button shows its computer-key mapping
and has a small LED (green for PLAY, amber for the rest) that follows both the computer
keyboard and the emulated firmware's real LED state. The note keys show their note names and
computer-key hints, and light amber while held or while the firmware plays them.

## Metronome

The metronome starts **off**; the rocker switch on its front panel starts and stops it. When
running, it sends a MIDI clock at **24 PPQN** (`0xF8`) to each emulator's emulated MIDI IN
jack. Switching on sends MIDI Start (`0xFA`), switching off sends MIDI Stop (`0xFC`), and
shutting down does the same. Tempo is 20–300 BPM: click the large monospace readout and type
a value, drag it, or use the grouped −/+ buttons. The small light next to it alternates on
every beat.

The clock thread runs independently of the GUI frame rate, so timing does not drift when
the window is idle. The emulators are paced to wall-clock time and their audio is mixed by
one `cpal` output stream (each slot has its own queue, so a stalled slot cannot block the
others).

## Building

* Rust 1.91 or newer, stable.
* Linux: ALSA headers (`libasound2-dev` on Debian/Ubuntu) and the usual X11/Wayland
  development packages for `eframe`.
* macOS/Windows: no extra system packages beyond the platform SDK.

The emulator crates from <https://github.com/joakimeriksson/fm1-emu> are used from git at
commit `9f0f65afc1b30c94daf295edc4b23ed71490849e`.

The FM-1 firmware images are unmodified packages from
[The FM-1 Firmware Hub](https://thefirmwarehub.com/) and M-VAVE. They keep their own
licences: SLOOP, Felucca and ChoralRoot are GPL-3.0; M-VAVE's FM-1_015 package is
Apache-2.0. The emulator crates are MIT. See `firmware/README.md`.

## Layout

```
src/app.rs       egui window: transport, two cards, dropdowns, keyboard routing
src/clock.rs     MIDI-clock thread, tempo, beat light, per-slot senders
src/emulator.rs  one thread + SoC + cores per slot, LCD/status/audio publication
src/audio.rs     two queues mixed into one cpal output stream
src/builtin.rs   embedded firmware catalogue (include_bytes!)
src/main.rs      startup and CLI firmware arguments
firmware/        the four embedded .fwsc packages (unmodified)
```
