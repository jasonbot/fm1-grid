# Port: eframe/egui → gpui-kit

> **Status: executed.** The port below landed; `eframe` is gone, the app runs on
> `gpui-kit = "=0.8.0"`, and headless UI tests (`src/tests/`) cover boot, pane
> selection, power-on with live LCD frames, and the metronome switch.
>
> Deviations from the plan as written:
> - The flash picker kept its search box after all: a `Combobox` state is created when
>   a device row's selector opens (dismiss clears it), instead of a plain menu.
> - The tempo readout is a themed `NumberInput` (built-in −/+ steppers, typed entry),
>   not the LCD-styled DragValue — the strict-theme call replaced the LCD look.
> - The firmware combobox rows render through the delegate's `render_item` override to
>   keep the file/SHA-256 provenance tooltip; it draws its own ✓ for the selection.
> - The sidebar takes a 32% width share (min 280 px, max 620 px) instead of egui's
>   exact "everything the grid leaves" split.
> - The lockfile was regenerated: enabling `test-support` pulls the target-gated
>   `gpui-fast-*` crates into the resolution graph, whose exact pins (cocoa 0.26.0,
>   unicode-properties 0.1.3, …) conflicted with the stale locked versions.
>
> Post-port fixes from first real use: `RenderImage` uploads must be BGRA (the atlas
> preserves the buffer; RGBA swapped red/blue); card LCDs are square-fit
> (`aspect_ratio` + `overflow_hidden`) so they can't spill past the card; an in-app
> `TitleBar` supplies window controls and dragging where the compositor has no
> decorations (GNOME); min window size 1024×700; the tick uploads only changed
> framebuffers and repaints only on change (plus a 0.5 s sweep), so an idle window
> renders ~2×/s; and `[profile.dev.package.fm1-*] opt-level = 3` keeps plain
> `cargo run` at realtime audio (unoptimized, the FM-1 core reaches only ~0.3x).

## Original plan

Replace the UI framework. The emulator stack (`fm1-core`/`fm1-soc` via the pinned
fm1-emu git rev) has no GUI dependency and is untouched, as are `emulator.rs`,
`clock.rs`, `audio.rs`, `routing.rs`, `hardware.rs`, `midi_keys.rs`, `affinity.rs` and
`builtin.rs` (~3,400 of 5,630 lines). The rewrite is the ~2,200 lines of egui UI.

**Theme rule (decided):** gpui-kit theme tokens everywhere. The only computed color
left in the app is the grid background (dominant color of the selected pane's screen,
eased over 500 ms). `ambient_tint`/`halo_color` are deleted; sidebar, devices pane,
cards and the selection ring use theme colors.

## Dependencies

- Remove `eframe = "0.33.3"`. Add `gpui-kit = "=0.8.0"` — exact pin, same policy as
  the pinned emulator rev: gpui-kit tracks Zed's fast-moving gpui snapshot
  (`gpui-pre` 0.3.9), expect churn on upgrades.
- Keep `rfd` (native file dialogs; framework-agnostic), `hqx`, `midir`, `cpal`, `libc`.
- Linux: gpui needs its own system packages (X11/Wayland dev libs, per gpui-kit's
  Installation doc) — verify before the first build.

## Architecture mapping

egui is immediate-mode: `update()` runs every frame and paints. gpui is retained:
`Render::render` builds an element tree; a repaint happens only after `cx.notify()`.

- `GridApp` keeps all non-UI state (slots, clock, hardware, routing, midi_keys, rings,
  master, powered, held, panel_down, leds, dominant…) minus every egui type
  (`TextureHandle`, `Color32`).
- New `GridView { app: GridApp, focus: FocusHandle, _tick: Task<()> }` entity,
  `impl Render for GridView` = window layout: flex column — central row
  (grid column + device sidebar) over the transport bar.
- The frame pump replaces `ctx.request_repaint_after(16ms)`: a `cx.spawn` loop
  (`cx.background_executor().timer(16ms).await` → `tick()` → `cx.notify()`). `tick()`
  is today's pre-paint work: `sync_held` merge, LCD frame uploads, `refresh_ghost`,
  LED decay, dominant-color ease clock.
- `main.rs`: `application().with_assets(assets::Assets).run(|cx| { init(cx);
  open_window(WindowOptions { 1180×1160, min 900×700 }, …) })`.

## Module by module


| Module | Today (egui) | gpui-kit replacement |
|---|---|---|
| `main.rs` | `eframe::run_native` | `gpui_kit::application()` + `init` + `open_window` |
| `app.rs` | `eframe::App::update`, 10 `TextureHandle`s | entity + `Render` + timer task; LCD frames as `ImageSource::Render(Arc<RenderImage>)` rebuilt when pixels change; bg ease via `Instant` math in `tick()` |
| `theme.rs` | palette consts, `apply_adwaita_style`, custom widgets | mostly deleted: `Switch`, `NumberInput`, `Button`, `Tooltip` components; cards = styled divs (`.bg(theme.card).border(theme.border).rounded()`). Keep only `luminance`/mix helpers for the computed grid bg. `load_system_sans` dropped (gpui text system) |
| `keys.rs` | `[(egui::Key, u8)]` tables | same tables re-typed to gpui keystroke strings (`"a"`, `"w"`, `";"`, `"quote"`, `"space"`, `"left"`…); panel layout tables unchanged |
| `grid.rs` | `CentralPanel`, manual rects, `interact` | card divs (3 rows × `h_flex`); `on_mouse_down` to select; active ring = `border_2` + `theme.primary`; container bg = the one computed color |
| `lcd.rs` | `ColorImage`/`TextureHandle`; pure pixel fns | pixel fns (`rgb565`, `dominant`, `ntsc`, `ghost`, `recede`, `upscaled`) unchanged in math, output RGBA bytes instead of `Color32`; `set_lcd`/`view_at` → per-frame `RenderImage` + `img()` inside a bezel div |
| `sidebar.rs` | child `Ui`, `ComboBox`, labels | `v_flex` divs; power `Switch`; firmware selector = `Combobox` (built-in search replaces the custom popup); status label; LCD `img`; `Scrollable` graph |
| `devices.rs` | rows, `toggle_switch`, `ProgressBar` | `Switch` + labels + `Button` + `Combobox` rows; `Progress` for flash %; `theme.danger` for errors |
| `graph.rs` | `ScrollArea`, chips, dropdowns | `Scrollable` + `Button`/`Combobox` rows |
| `transport.rs` | `TopBottomPanel`, tempo stepper, LED, port picker | bottom bar div; tempo = `NumberInput` (or two-button stepper); beat LED = tiny canvas quad; MIDI port = `Select` |
| `panel.rs` | `painter.text/rect`, interact rects, wheel | custom canvas element (`paint_quad`/`paint_path` for body, keys, buttons); hit regions = absolutely-positioned divs with `on_mouse_down/up`, or one canvas + position math; wheel = `on_scroll_wheel` |
| `firmware_picker.rs` | `ComboBox` + `TextEdit` filter | deleted — `Combobox` has search; keep `matches()` only if tag matching is wanted beyond substring |

## Input model (the one semantic rewrite)

egui polls `input.key_down(key)` per frame; gpui delivers events.

- Root view: `track_focus(&focus)` (tab-stop, focused on open), `.on_key_down` /
  `.on_key_up` maintain a `HashSet` of held keys; `tick()` merges it into `want[]`
  exactly like `sync_held` does today.
- Text inputs (`Combobox` filter, `NumberInput`) consume their keystrokes and stop
  propagation, so "typing must not play notes" survives structurally — replaces the
  `ctx.wants_keyboard_input()` guard.
- Window blur delivers no key-up for already-released keys → clear the held set on
  focus-out (today egui just stops reporting them).
- Encoder keys (arrows, comma, period) → key-down handlers calling `turn_encoder`.
  OS key-repeat arrives as repeated `KeyDownEvent`s; matches today's
  `input.key_pressed` behavior.

## Color mapping (strict theme)

- Stays computed: grid background = `dominant()` of the active pane's screen, eased
  500 ms (manual channel lerp in `tick()`, replacing `animate_value_with_time`).
- Everything else → `cx.theme()` tokens: `CARD_BG`/`FAINT_BG` → `theme.card` /
  `theme.background`; `INK`/`DIM` → `theme.foreground` / `theme.muted_foreground`;
  `ACCENT` → `theme.primary` (also the selection ring, replacing `halo_color`);
  `RED` → `theme.danger`; `BORDER`/`PANEL_EDGE` → `theme.border`.
- **Fixed by veto only:** the FM-1 hardware colors (`PANEL_BODY`, `SILK`, keycap
  ivory, `AMBER`/`GREEN` indicators, LCD on/off green) depict the physical machine,
  like its screen pixels. Plan keeps them fixed; say the word to flatten them to
  theme too.

## LCD rendering path (the perf spike)

`ImageSource::Render(Arc<RenderImage>)` takes CPU pixel buffers (Zed uses it for
video frames). Per slot, rebuild the `RenderImage` when `upload_lcds` produces new
pixels; cards and the sidebar render `img(source)`; the bezel is a padded dark div.
Verify in the spike: nearest-neighbor scaling (the sidebar already hqx-upscales
CPU-side to exact 2x/4x, so filtering is moot there) and image-cache churn at
60 Hz × 10 surfaces (~230 KB per 240×240 frame). Fallback: `ImageSource::Custom`
closure resolving the latest frame at paint time, or a quad `canvas`.

## File dialogs

`rfd` stays — native OS picker, called from click handlers; result updates the
entity + `cx.notify()`.

## Order of work

1. **Spike:** minimal gpui-kit window — one LCD framebuffer at 60 Hz, eased computed
   bg, one `Switch`. Validates the image path, timer pump, theme tokens.
2. **Skeleton:** window + layout regions + theme, `GridApp` ported to gpui types.
3. **Read path:** grid cards, LCD images, power switch, firmware `Combobox`, status.
4. **Devices + graph:** `Switch`/`Button`/`Combobox`/`Progress`/`Scrollable`.
5. **Transport + panel:** canvas painting, click/wheel handlers, held-key set, tempo.
6. **Polish:** Respect light/dark system theme settings, use default gpui-kit theme colors, tooltips, blur/repeat edge cases, README update
   (it currently documents eframe).

Verification per phase: build + run the window and exercise the path (power a pane,
play keys, watch flash progress). No test suite exists; smoke = run the app.

## Risks

- LCD image swap at 60 Hz (allocation/cache churn) — de-risked first in the spike.
- Held-key edge cases (blur, OS repeat) — new code, no egui equivalent to copy.
- gpui-kit 0.8 / gpui-pre 0.3.9 churn — pinned exact; upgrades are a deliberate act.
