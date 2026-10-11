# ViMouse

Effectively control your cursor with only a keyboard.

Supports macOS, Windows, and Linux.

<details>
<summary>Table of Contents</summary>

- [Demo](#demo)
- [Usage](#usage)
  - [Cursor Movement](#cursor-movement)
  - [Scrolling](#scrolling)
  - [Clicking](#clicking)
  - [Jump Grid](#jump-grid)
  - [Marks](#marks)
  - [Other Controls](#other-controls)
- [Installation](#installation)
  - [Option 1: Download a Release (fastest)](#option-1-download-a-release-fastest)
  - [Option 2: Build From Source](#option-2-build-from-source)
    - [Prerequisites](#prerequisites)
    - [macOS](#macos)
    - [Windows / Linux](#windows--linux)
  - [Limitations](#limitations)
  - [Platform Notes](#platform-notes)
- [License](#license)

</details>

## Demo

https://github.com/user-attachments/assets/9f66ab8f-f457-4b9f-b1b3-5b6602156a6b

- Note: The green circle around the mouse cursor was edited in to highlight the position of the cursor; it is not part of the overlay.

## Usage

ViMouse has two modes, toggled like Vim:

| Mode | Key | Indicator | Description |
|------|-----|-----------|-------------|
| **Normal** | `CapsLock` | Blue | ViMouse intercepts keys (cursor control active) |
| **Insert** | `i` | Green | Keys pass through to apps normally |

A thin line along the bottom edge of your screen shows the current mode by color. Press `Comma` to toggle its visibilty.

**Unless otherwise specified, ViMouse keybinds only work in Normal mode; Insert mode is reserved for typing.**

> [!NOTE]
> All keybinds can be configured in `src/config.rs`. The builds available in [Releases](https://github.com/ExxML/ViMouse/releases/latest) use the default configuration outlined in this README. To use a custom configuration, you must [build from source](#option-2-build-from-source).

---

### Cursor Movement

Hold `H` / `J` / `K` / `L` to move the cursor:

```
        K
        ↑
   H ←     → L
        ↓
        J
```

| Modifier | Effect |
|----------|--------|
| `Space` (hold) | 5× speed |
| `Left Alt` (hold) | 0.5× speed |

Tap a move key to move the cursor 50 logical pts/sec. Hold to move 300 logical pts/sec.
- Speeds are in logical points (DPI-normalized) so the cursor feels the same across monitors of differing DPI.

Mouse speed values are configurable in `src/config.rs`. Feel free to play around with whatever settings feel right to you. 

For example:
- If you want to disable mouse acceleration, set `CURSOR_ACCELERATION` to 0.
- If you want to use two-speed movement (current default), set `CURSOR_ACCELERATION` to `f64::INFINITY` and tweak `CURSOR_MAX_SPEED` to your preference.
- If you want normal mouse acceleration, set `CURSOR_ACCELERATION` to a reasonable logical pts/sec² value.

> [!TIP]
> Hold two movement keys simultaneously to move diagonally.

---

### Scrolling

Hold `Left/Right Shift` + `H` / `J` / `K` / `L` to scroll.

Scrolling features the same `Space` / `Left Alt` speed modifiers as [Cursor Movement](#cursor-movement).

Scrolling holds a steady 8 units/sec for the first 3 seconds, then accelerates by 20 units/sec² for as long as you keep holding.

Feel free to configure the scroll speed values in `src/config.rs`.

---

### Clicking

| Key | Action |
|-----|--------|
| `;` | Left click |
| `'` | Right click |
| `M` | Middle (scroll) click |
| `U` | Back (X1) click |
| `O` | Forward (X2) click |

---

### Jump Grid

The screen is divided into a 5×3 grid - press the labeled key to teleport the cursor to that cell's center.

```
┌───────┬───────┬───────┬───────┬───────┐
│   Q   │   W   │   E   │   R   │   T   │
├───────┼───────┼───────┼───────┼───────┤
│   A   │   S   │   D   │   F   │   G   │
├───────┼───────┼───────┼───────┼───────┤
│   Z   │   X   │   C   │   V   │   B   │
└───────┴───────┴───────┴───────┴───────┘
```
- Each cell is also divided into a 5×3 grid. Press a second jump grid key to jump to a subcell within the current cell. By default the subcell jump stays primed indefinitely; set `JUMP_GRID_DELAY` to a number of seconds to time it out, or to 0.0 to disable subcell jumps.
- Press `Period` to toggle a reference jump grid overlay to serve as a guide for where to jump.
- Press `Slash` to toggle a reference grid of letters that show where you will jump for each letter.

Press `n` to cycle focus to another monitor, moving the cursor, mode line, and jump grid.

> [!TIP]
> It is recommended to use the jump grid as your primary method of navigation and only use the cursor movement keys (HJKL) for micro-adjustments.
> If you wish to use the reference grid overlay or grid letters, there are many customization options available in `src/config.rs`.

---

### Marks

Marks are custom cursor positions you set and jump back to, like Vim marks. A label marks each position on the screen, showing which key jumps where.

- Press a number key `0`-`9` to set a mark at the current cursor position.
- Press that same key again to jump the cursor to the marked position.
- Hold `Left Shift` and press a mark key to remove that mark.
- Hold `Left Shift` + `` ` `` to remove all marks.

---

### Shortcuts

In Insert mode, hold `Left Alt` and press:

- `H` / `J` / `K` / `L` to send the Left / Down / Up / Right arrow keys.
- `U` / `O` to send Home / End.

Other held modifiers carry over (e.g. `Ctrl + Left Alt + H` sends `Ctrl + Left`).

---

### Other Controls

These keybinds are available in both Normal and Insert modes.

| Key | Action |
|-----|--------|
| `Right Alt` | Toggle ViMouse overlay |
| `Left Ctrl + Left Alt + Left Shift + Q` | Quit ViMouse |

## Installation

### Option 1: Download a Release (fastest)

1. Download the compressed binary for your OS and architecture from [Releases](https://github.com/ExxML/ViMouse/releases/latest).
2. Extract and run the executable (`ViMouse.app` on macOS, `vimouse.exe` on Windows, `vimouse` on Linux).

See [Limitations](#limitations) and [Platform Notes](#platform-notes) for permissions and known limitations.

### Option 2: Build From Source

#### Prerequisites

Install [Rust toolchain](https://rustup.rs) (stable).

#### macOS

Use [`cargo-bundle`](https://github.com/burtonageo/cargo-bundle) to produce a native app bundle:

```bash
cargo install cargo-bundle
cargo bundle --release
```

Run from `target/release/bundle/osx/ViMouse.app`.

Bundling is preferred over a plain Unix executable because the latter opens a terminal window on launch. If you prefer, `cargo build --release` works as well.

#### Windows / Linux

```bash
cargo build --release
```

Run from `target/release/vimouse.exe` (Windows) or `target/release/vimouse` (Linux).

### Limitations

ViMouse shares the keyboard with your OS and apps, so some key combinations behave in ways you might not expect.

**Modifier keys**
- **Modifier order matters.** ViMouse handles a modifier combo based on the *first* modifier you press. To combine a modifier with scrolling, press `Shift` first: `Shift` → `Ctrl` → `J` sends Ctrl+scroll down, but `Ctrl` → `Shift` → `J` sends `Ctrl+Shift+J` to the focused app (e.g. opens Chrome's DevTools console). The same goes for `Cmd`/`Win`, and for `Left Alt` slow movement.
- **Holding a modifier pauses most keybinds.** While `Ctrl`, `Alt`, `Shift`, or `Cmd`/`Win` is held, keys go to the focused app so shortcuts like `Ctrl+C` work. Only `H`/`J`/`K`/`L` (when `Shift` or `Left Alt` was pressed first), left/right click, and unmarking stay with ViMouse. Everything else (jump grid, marks, `i`, `n`, overlay toggles, middle/back/forward click) goes to the app: `Shift` + `Q` types `Q` into a text field, and `Ctrl` + `M` never middle-clicks.
- **`Shift` and `Left Alt` pressed mid-action are hidden from apps.** If you press either while holding another ViMouse key (e.g. while moving or holding a click), apps don't see it until you physically release it, even after you stop moving or switch to Insert mode (hold `J`, press `Shift`, release `J`, press `i`, and typed letters come out lowercase). For Shift+click or Alt+click, press the modifier before the click key. `Ctrl` and `Cmd`/`Win` are never hidden.
- **Modifier+drag is limited.** With `Shift` held, `H`/`J`/`K`/`L` scroll instead of move, and `Left Alt` is hidden from apps while the cursor moves, so Shift+drag and Alt+drag aren't possible. For Ctrl+drag (or Cmd+drag), hold a move key *before* pressing `Ctrl`, then the click key; pressing `Ctrl` first sends the move keys to the app.
- **Extra `Shift`/`Alt` events.** To keep scrolls and slow movement clean, ViMouse sends a synthetic release of a held `Shift`/`Left Alt` when movement starts and a synthetic press when it stops. Apps that react to bare modifier taps (e.g. double-Shift search in JetBrains IDEs) may trigger.

**Reserved keys**
- **`Space` is always captured in Normal mode**, even with a modifier held: `Cmd+Space` (Spotlight), `Ctrl+Space`, `Alt+Space`, and `Win+Space` only work in Insert mode.
- **`;` and `'` are always captured in Normal mode**: `Ctrl` + `;` sends Ctrl+left click, never `Ctrl+;`.
- **`Left Alt` + `H`/`J`/`K`/`L`** slow-move in Normal mode, and in Insert mode `Left Alt` + `H`/`J`/`K`/`L`/`U`/`O` are always [shortcuts](#shortcuts), even with other modifiers held. App shortcuts on those combos and macOS Option characters (e.g. `Option+U` umlaut, `Option+O` → `ø`) are unavailable.
- **`Right Alt` (AltGr) toggles the overlay in both modes** and only reaches apps if another modifier is held first, so it can't be used as a second Alt key or, on macOS (`Right Option`) and Linux, to type special characters. On Windows layouts with AltGr (e.g. German), AltGr arrives with a fake `Ctrl` press, so it types normally but can't toggle the overlay.
- **`Caps Lock` only enters Normal mode.** It is always captured (in both modes, with any modifier) and its toggle is turned off at launch, so it can't type capitals or switch input sources.
- **`Left Ctrl + Left Alt + Left Shift + Q` always quits ViMouse**, so apps never receive it.
- **Unbound keys do nothing in Normal mode.** Letters, numbers, and punctuation without a ViMouse binding are swallowed. These always pass through: `Enter`, `Esc`, `Tab`, `Backspace`, `Delete`, `Insert`, `Home`/`End`, `Page Up`/`Page Down`, arrows, `F1`–`F12`, numpad, `Print Screen`, `Scroll Lock`, `Pause`, `Num Lock`, and media keys.

**Jump grid and marks**
- **The next grid key after a cell jump is always a subcell jump**, even minutes later. To jump to a different cell, press any other ViMouse key first (e.g. tap a move key), or set `JUMP_GRID_DELAY` to a timeout. Moving the physical mouse doesn't reset it.
- **Unmarking needs `Left Shift` alone.** `Right Shift` + `1` types `!`, any extra modifier sends the combo to the app, and a `Left Shift` pressed while another ViMouse key is held is hidden (see above), so the mark key sets/jumps instead of unmarking.

**Platform-specific**
- **Windows: bindings follow your layout's letters, not physical positions.** On AZERTY, QWERTZ, Dvorak, etc. each binding moves to wherever its letter is (e.g. the jump grid is scrambled on AZERTY). macOS and Linux bind by physical QWERTY position.
- **`Right Win` (Windows) and `Fn`/Globe (macOS) aren't recognized as modifiers.** In Normal mode, `Right Win` + key and Globe + key (e.g. Globe+E emoji picker) trigger the ViMouse binding instead of the OS shortcut. Use `Left Win`, or switch to Insert mode.

### Platform Notes

**Hardware**
- While using ViMouse, some (less advanced) keyboards may exhibit ghosting behaviour caused by using a simple row/column matrix rather than diodes on every key switch. This means that certain three-key combinations may fail to register, causing some ViMouse keybindings to not work. This is a hardware limitation and there is unfortunately no possible software fix for this.

**macOS**
- Release builds are ad-hoc signed but **not** notarized (no Apple Developer ID). On first launch, macOS will block the app with an *"unidentified developer"* / *"cannot verify free of malware"* prompt. To open it, either:
  - Right-click `ViMouse.app` → **Open**, then confirm (on macOS Sequoia and later, go to *System Settings → Privacy & Security* and click **Open Anyway**), or
  - Clear the quarantine attribute from Terminal: `xattr -dr com.apple.quarantine /path/to/ViMouse.app`
- ViMouse requires Accessibility permission to intercept input. On first launch, it will prompt you to grant it under *System Settings → Privacy & Security → Accessibility*.

**Windows**
- Release builds are not signed with a trusted code-signing certificate (no EV/OV code signing cert). On first launch, Windows Defender will block the app with a *"Windows protected your PC"* prompt. To open it anyway, click **More info**, then **Run anyway**.
- ViMouse must be launched with administrator privileges to interact with admin-level processes, such as Task Manager, UAC, Command Prompt, etc.
  - You can use Task Scheduler to run vimouse.exe with elevated privileges on PC startup.
- Windows has a known bug where the mouse cursor disappears after waking up from sleep. The cursor will only reappear when the mouse is physically moved; ViMouse cannot wake the cursor (but can still control it). This bug can be programmatically circumvented but is more bloat than it's worth.

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).

<br>

<div align="center">
    <small>f*ck mice</small>
</div>
