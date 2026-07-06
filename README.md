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
| **Normal** | `CapsLock` | `N` | ViMouse intercepts keys (cursor control active) |
| **Insert** | `i` | `I` | Keys pass through to apps normally |

A small icon in the bottom-left corner of your screen shows the current mode.

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
| `Space` (hold) | 3× speed |
| `Left Alt` (hold) | 0.3× speed |

Tap a move key to move the cursor 100 px/sec. Hold to move 500 px/sec.

All values and configurations are modifiable in `src/config.rs`. Feel free to play around with whatever settings feel right to you. 

For example:
- If you want to disable mouse acceleration, set `CURSOR_ACCELERATION` to 0.
- If you want to use two-speed movement (current default), set `CURSOR_ACCELERATION` to `f64::INFINITY` and tweak `CURSOR_MAX_SPEED` to your preference.
- If you want normal mouse acceleration, set `CURSOR_ACCELERATION` to a reasonable px/sec² value.

> [!TIP]
> Hold two movement keys simultaneously to move diagonally.

---

### Scrolling

Hold `Left Shift` + `H` / `J` / `K` / `L` to scroll.

Scrolling features the same `Space` / `Left Alt` speed modifiers as [Cursor Movement](#cursor-movement).

There is no scroll acceleration by default, but this can be modified in `src/config.rs`.

---

### Clicking

| Key | Action |
|-----|--------|
| `;` | Left click |
| `'` | Right click |
| `M` | Middle (scroll) click |
| `O` | Back (X1) click |
| `P` | Forward (X2) click |

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
- Each cell is also divided into a 5×3 grid. Press a second jump grid key within 1 second to jump to a subcell within the current cell.
- Press `Slash` to toggle a reference jump grid overlay to serve as a guide for where to jump.
- Press `Period` to toggle a reference grid of letters that show where you will jump for each letter.

Press `n` to cycle focus to another monitor, moving the cursor, mode icon, and jump grid.

> [!TIP]
> It is recommended to use the jump grid as your primary method of navigation and only use the cursor movement keys (HJKL) for micro-adjustments.
> If you wish to use the reference grid overlay or grid letters, there are many customization options available in `src/config.rs`.

---

### Marks

Marks are custom cursor positions you set and jump back to, like Vim marks. A label marks each position on the screen, showing which key jumps where.

- Press a number key `0`-`9` to set a mark at the current cursor position.
- Press that same key again to jump the cursor to the marked position.
- Hold `Left Shift` and press a mark key to remove that mark.
- Press `Left Shift` + `` ` `` to remove all marks.

---

### Other Controls

These keybinds are available in both Normal and Insert modes.

| Key | Action |
|-----|--------|
| `Right Shift` | Toggle ViMouse overlay |
| `Left Ctrl + Left Alt + Left Shift + Q` | Quit ViMouse |

## Installation

### Option 1: Download a Release (fastest)

1. Download the compressed binary for your OS and architecture from [Releases](https://github.com/ExxML/ViMouse/releases/latest).
2. Extract and run the executable (`ViMouse.app` on macOS, `vimouse.exe` on Windows, `vimouse` on Linux).

See [Platform Notes](#platform-notes) for permissions and known limitations.

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
