# 🖱️ViMouse

Effectively control your cursor with only a keyboard.

Supports macOS, Windows, and Linux.

## Usage

> [!NOTE]
> All keybinds mentioned below are configurable in `src/config.rs`.

ViMouse has two modes, toggled like Vim:

| Mode | Key | Indicator | Description |
|------|-----|-----------|-------------|
| **Normal** | `CapsLock` | `N` | ViMouse intercepts keys (cursor control active) |
| **Insert** | `i` | `I` | Keys pass through to apps normally |

A small overlay badge in the bottom-left corner of your screen shows the current mode.

While in Normal mode, only ViMouse keybinds are suppressed; all other keys pass through as usual.

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
| `Space` (hold) | 2× speed |
| `Alt` (hold) | 0.3× speed |

Movement accelerates the longer you hold: starts at 100 px/s and ramps up to 2400 px/s. 

All these values are modifiable in `src/config.rs`. For example, if you want to disable acceleration, set `ACCELERATION` to 0. If you want to enable automatic two-speed movement, set `ACCELERATION` to `f64::INFINITY` and tweak `MAX_SPEED` to your preference.

> [!TIP]
> Hold two movement keys simultaneously to move diagonally.

---

### Scrolling

Hold `Left Shift` + `H` / `J` / `K` / `L` to scroll.

Like cursor movement, scrolling also features acceleration and `Space` / `Alt` speed modifiers.

---

### Clicking

| Key | Action |
|-----|--------|
| `;` | Left click |
| `'` | Right click |

---

### Jump Grid

The screen is divided into a 5×3 grid - press the labeled key to warp the cursor to that cell's center.

```
┌───────┬───────┬───────┬───────┬───────┐
│   Q   │   W   │   E   │   R   │   T   │
├───────┼───────┼───────┼───────┼───────┤
│   A   │   S   │   D   │   F   │   G   │
├───────┼───────┼───────┼───────┼───────┤
│   Z   │   X   │   C   │   V   │   B   │
└───────┴───────┴───────┴───────┴───────┘
```
- Press `Right Shift` to toggle a reference grid overlay to serve as a guide for where to jump.

Press `n` to cycle focus to another monitor, moving the cursor, mode icon, and jump grid.

---

### Other Controls

| Key | Action |
|-----|--------|
| `Ctrl + Shift + Q` | Quit ViMouse |

## Installation

### Prerequisites

- [Rust toolchain](https://rustup.rs/) (stable)

### macOS

Uses [`cargo-bundle`](https://github.com/burtonageo/cargo-bundle) to produce a native app bundle:

```bash
cargo install cargo-bundle
cargo bundle --release
```
- Bundling is preferred over running a plain Unix executable as a terminal window opens on launch by default. If you would rather, `cargo build --release` works fine as well.

This program can be ran from `target/release/bundle/osx/ViMouse.app`.
- ViMouse requires Accessibility permission to intercept input. On first launch, ViMouse will prompt you to grant it under **System Settings → Privacy & Security → Accessibility**.

### Windows / Linux

```bash
cargo build --release
```

This program can be ran from `target/release/vimouse.exe`.

> [!NOTE]
> Known Limitations on Windows:
> - Unless ViMouse is launched with administrator privileges, it cannot interact with admin-level processes, such as Task Manager, UAC, Command Prompt, etc.
>   - You can use Task Scheduler to run the .exe with elevated privileges on startup.
> - Windows has a known issue that the mouse cursor disappears after waking up from sleep. Only physically moving the mouse or touching the trackpad will make the cursor appear again. This can be solved programmatically but is likely more bloat than it's worth.

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).

<br>

<div align="center">
    <h6>f*ck mice</h6>
</div>
