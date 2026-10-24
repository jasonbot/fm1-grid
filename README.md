# FM-1 Grid

A 2×2 grid of four emulated M-VAVE FM-1s, each pane with its own firmware and LCD, with a
gpui-kit interface (themed cards and controls from the GPUI component library). Emulators
are powered **off** at startup — flip the power switch
in the device sidebar to start one, so idle panes cost nothing. Click anywhere in a pane to
route the computer keyboard there; the selected card gets a blue border. Every grid screen
always shows at 1x: the selected pane's live screen appears in the device sidebar
**hqx-upscaled at an exact 4x, 2x or 1x** (the largest that fits), while its grid card
shows an NTSC-distorted snapshot refreshed in realtime. The device sidebar
is greedy for all space right of the grid and stacks, from the top: a large
`Device N: Firmware Name` heading, the 4x screen (horizontally centered), the power switch,
firmware selector and
status, and the placeholder for firmware-specific controls. The grid area's background takes
the dominant colour of the selected pane's screen (recalculated once a second, fading over
500 ms); the sidebar's background is the same colour slightly desaturated and strongly
lightened, so its text and controls stay readable. The large, clickable FM-1 front panel
above the metronome mirrors the active pane: its keys and buttons can be played with the
mouse, its knobs turn with the −/+ ends, and its LEDs show the firmware's own state as well
as what the computer keyboard is doing.

```
┌────────────┐ ┌────────────┐  ┌────────────────────────────┐
│ ┌────────┐ │ │ ┌────────┐ │  │ Device 1: SLOOP 2.8        │
│ │  LCD   │ │ │ │  LCD   │ │  │ ┌────────────────────────┐ │
│ └────────┘ │ │ └────────┘ │  │ │      live 4× LCD       │ │
└────────────┘ └────────────┘  │ └────────────────────────┘ │
┌────────────┐ ┌────────────┐  │ [ ○─── ] [ SLOOP 2.8 ▾ ]   │
│ ┌────────┐ │ │ ┌────────┐ │  │ Off                        │
│ │  LCD   │ │ │ │  LCD   │ │  │ No custom behavior for …   │
│ └────────┘ │ │ └────────┘ │  └────────────────────────────┘
└────────────┘ └────────────┘
   2 rows × 2 columns
┌─ FM-1 PANEL ────────────────────────── CLICKABLE · KEYBOARD LIGHTS · PANE 1 ──┐
│ [− MASTER +]  [− SELECT +]                 [− KNOB 1 +] … [− KNOB 4 +]       │
│ [− PRESETS +] [− ALGORITHM +]                                               │
│ [OCT− Z] [OCT+ X]   [FX 1][SCL 2][ENV 3][LFO 4][EDIT 5][GLO 6]              │
│                     [HOME 7][SAVE 8][ARP 9][SEQ 0][PLAY ␣][REC R]            │
│ [F3 C][F#3 V][G3 B] … [C4 A][C#4 W][D4 S] … [F5 '][F#5][G5]                 │
└──────────────────────────────────────────────────────────────────────────────┘
[ ○─── ] MIDI CLOCK  120 BPM [ − ][ + ]  ● BEAT
         STOPPED · 24 PPQN
```

## Run

```sh
cargo run --release
```

With no arguments the first panes load the embedded firmwares (powered off until you
switch them on):

| Pane | Firmware |
|------|----------|
| 1 | SLOOP 2.8 |
| 2 | Felucca 1.6.1 |
| 3 | FM1 Open V15 |
| 4 | ChoralRoot 0.26.1 |

Any pane can load any of the embedded firmwares from the firmware selector in the device
sidebar. On Linux each running emulator thread is pinned to its own physical core (one logical
CPU per core) while cores last; panes beyond the core count float. Four emulators at once
want four cores — power on only what you play.

Pass up to four firmware files to choose what loads where (left to right, top to bottom):

```sh
cargo run --release -- sloop.fwsc felucca.fwsc
```

A `.fwsc` file is treated as an update package; a `.bin` or `.xip` file is treated as a raw
app image mapped at XIP base, entered at `0x0200_0120`. `--help` lists the options.

### Firmware selector

The cards are display-only monitors with no labels of their own, all screens always at 1x.
The selected pane's power switch, firmware selector and status live at the top of the device
sidebar, under the `Device N:` heading; its live screen shows there hqx-upscaled at an
exact pixel multiple, centered, while
its grid card shows an NTSC-distorted snapshot refreshed in realtime. Emulators are off by
default; the
power switch starts and stops the selected pane's emulator — powering off stops its thread,
silences its audio and blanks its screen, powering on boots whatever firmware the pane has
loaded. The selector is only enabled while the pane is powered off; while it is on, the
selector is greyed out and hovering it explains why. It offers the embedded firmwares and
**Other…**, which opens a file picker. The embedded images are compiled into the binary
(see `firmware/README.md` for versions, sources and SHA-256 hashes). Hovering a choice
shows its file name and hash. Choosing a firmware swaps what the pane has loaded; it boots
the next time the power switch is turned on, and the metronome is reconnected automatically.

The embedded list lives in `firmware/manifest.txt` and is compiled in at build time. Add or
remove entries with the helper script, then rebuild:

```sh
scripts/firmware.sh add ~/Downloads/new-1.0.fwsc "New Firmware" 1.0
scripts/firmware.sh del "New Firmware"
scripts/firmware.sh list
```

`add` copies the package into `firmware/` and records its SHA-256; `del` removes both the
entry and the file. Manifest order is the default slot order: the first lines are what
starts with no CLI arguments.

## Keyboard

The active pane receives the same layout as `fm1-live`, plus the low octave on the seven
letters nothing else claims:

| Keys | FM-1 |
|------|------|
| `A W S E D F T G Y H U J K O L P ; '` | C4 … F5 (key IDs 21–38) |
| `C V B N M Q I` | F3, F#3, G3, G#3, A3, A#3, B3 (IDs 14–20) |
| `Z` / `X` | octave down / up (IDs 0, 1) |
| `1 2 3 4 5 6 7 8 9 0` | function buttons (IDs 2–11) |
| `Space` | PLAY / PAUSE (ID 12) |
| `R` | REC (ID 13) |
| `←` / `→` | PRESETS encoder −/+ |
| `↑` / `↓` | ALGORITHM encoder −/+ |
| `,` / `.` | SELECT encoder |

Keys are sent as key-down/key-up events to the active emulator. Switching panes releases
held keys on the old one, so no note sticks. Clicking anywhere in a pane's box makes it
active; the blue outline shows which pane has the keyboard. A powered-off pane ignores
keys until its power switch is turned on.

The FM-1 panel is also a control surface. The 27 white and black note keys are tall and can
be clicked and held. MASTER, SELECT, PRESETS and ALGORITHM sit in a block at the upper left;
KNOB 1–4 are to their right. Each of those controls is a button group with a small −/+ end
on the left and right that turns it one detent. Each button shows its computer-key mapping
and has a small LED (green for PLAY, amber for the rest) that follows both the computer
keyboard and the emulated firmware's real LED state. The note keys show their note names and
computer-key hints, and light amber while held or while the firmware plays them.

## MIDI graph

The sidebar's MIDI graph connects sources to any number of sinks: system MIDI ports, the
running panes and live FM-1 units. Pick a source with **+ connect a MIDI out…**, then add
sinks with a row's **+**; clicking a chip removes that connection. Live units are
bidirectional — their firmwares advertise one USB-MIDI in/out pair over USB-C.

A running pane is a sink only, and that is the emulator's limit rather than the
firmware's: the FM-1's TRS jack is MIDI-in alone, and the firmwares' real MIDI path is
USB — every bundled hub firmware advertises a bidirectional USB-MIDI interface (checked
at load, from the image's descriptors), which the emulated SoC cannot enumerate, and the
UART's TX side is unwired. A pane's MIDI out therefore stays silent; the graph says so
while such a pane runs. (M-VAVE's stock FM1 Open V15 is the one bundled firmware that
advertises no USB-MIDI at all.)

## Metronome

The metronome starts **off**; the rocker switch on its front panel starts and stops it. When
running, it sends a MIDI clock at **24 PPQN** (`0xF8`) to each emulator's emulated MIDI IN
jack. Switching on sends MIDI Start (`0xFA`), switching off sends MIDI Stop (`0xFC`), and
shutting down does the same. Tempo is 20–300 BPM: type a value in the readout or use its
−/+ steppers. The small light next to it alternates on every beat.

The clock thread runs independently of the GUI frame rate, so timing does not drift when
the window is idle. The emulators are paced to wall-clock time and their audio is mixed by
one `cpal` output stream (each slot has its own queue, so a stalled slot cannot block the
others).

## Building

* Rust 1.91 or newer, stable.
* Linux: ALSA headers (`libasound2-dev` on Debian/Ubuntu) and the usual X11/Wayland
  development packages for gpui (see the gpui-kit installation docs).
* macOS/Windows: no extra system packages beyond the platform SDK.

The emulator crates from <https://github.com/joakimeriksson/fm1-emu> are used from git at
commit `9f0f65afc1b30c94daf295edc4b23ed71490849e`.

The FM-1 firmware images are unmodified packages from
[The FM-1 Firmware Hub](https://thefirmwarehub.com/) and M-VAVE. They keep their own
licences: SLOOP, Felucca and ChoralRoot are GPL-3.0; M-VAVE's FM-1_015 package is
Apache-2.0. The emulator crates are MIT. See `firmware/README.md`.

## Layout

```
build.rs         generates the embedded firmware table from firmware/manifest.txt
scripts/firmware.sh  add/del/list for the embedded firmware list
src/affinity.rs  Linux physical-core detection and per-emulator pinning
src/app.rs       GridApp state and lifecycle: pane power, firmware install, keyboard and
                 encoder routing, per-tick LCD upload, the gpui root view
src/clock.rs     MIDI-clock thread, tempo, beat light, per-slot senders
src/emulator.rs  one thread + SoC + cores per slot, LCD/status/audio publication
src/audio.rs     four queues mixed into one cpal output stream
src/builtin.rs   embedded firmware catalogue (includes the generated table)
src/grid.rs      the 2x2 grid of pane cards and the computed background colour
src/sidebar.rs   the device sidebar: heading, upscaled screen, pane attributes
src/panel.rs     the miniature FM-1 front panel control surface
src/transport.rs the bottom bar: front panel plus the metronome row
src/lcd.rs       RGB565 decode, dominant colour, ghost/recede/hqx pictures, frame images
src/keys.rs      FM-1 control maps: keyboard layout, panel key/button tables
src/theme.rs     the FM-1 hardware colours (panel, keycaps, LEDs); app chrome follows
                 the gpui-kit theme
src/main.rs      startup and CLI firmware arguments
src/lib.rs       the library root the binary and the UI tests share
src/tests/       headless UI smoke tests (gpui-kit test-support)
firmware/        the embedded .fwsc packages (unmodified) plus manifest.txt
```
