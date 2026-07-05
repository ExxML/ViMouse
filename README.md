# ViMouse

Effectively control your cursor with only a keyboard.

Supports macOS, Windows, and Linux.

## Demo

https://github.com/user-attachments/assets/9f66ab8f-f457-4b9f-b1b3-5b6602156a6b

- Note: The green circle around the mouse cursor was edited in to highlight the position of the cursor; it is not part of the overlay.

## Usage

> [!NOTE]
> All keybinds mentioned below are configurable in `src/config.rs`.

ViMouse has two modes, toggled like Vim:

| Mode | Key | Indicator | Description |
|------|-----|-----------|-------------|
| **Normal** | `CapsLock` | `N` | ViMouse intercepts keys (cursor control active) |
| **Insert** | `i` | `I` | Keys pass through to apps normally |

A small icon in the bottom-left corner of your screen shows the current mode.

**Unless otherwise specified, ViMouse keybinds only work in Normal mode; Insert mode is reserved for typing.**

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

> [!IMPORTANT]
> Some (less advanced) keyboards may exhibit ghosting behaviour- they use a simple row/column matrix without diodes on each key switch. This means that certain three-key combinations may fail to register, causing some ViMouse keybindings to not work. This is a hardware limitation and there is unfortunately no possible software fix for this.

### Prerequisites

- [Rust toolchain](https://rustup.rs) (stable)

### macOS

Uses [`cargo-bundle`](https://github.com/burtonageo/cargo-bundle) to produce a native app bundle:

```bash
cargo install cargo-bundle
cargo bundle --release
```
Bundling is preferred over running a plain Unix executable because a terminal window opens on launch by default. If you would rather, `cargo build --release` works fine as well.

This program can be ran from `target/release/bundle/osx/ViMouse.app`.
- ViMouse requires Accessibility permission to intercept input. On first launch, ViMouse will prompt you to grant it under **System Settings → Privacy & Security → Accessibility**.

### Windows / Linux

```bash
cargo build --release
```

This program can be ran from `target/release/vimouse.exe`.

> [!IMPORTANT]
> Known Limitations on Windows:
> - Unless ViMouse is launched with administrator privileges, it cannot interact with admin-level processes, such as Task Manager, UAC, Command Prompt, etc.
>   - You can use Task Scheduler to run the .exe with elevated privileges on startup.
> - Windows has a known issue that the mouse cursor disappears after waking up from sleep. Only physically moving the mouse or touching the trackpad will make the cursor appear again. This can be solved programmatically but is likely more bloat than it's worth.

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).

<br>

<div align="center">
    <small>f*ck mice</small>
</div>
