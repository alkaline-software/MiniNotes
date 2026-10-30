# Probing

How to look at a state of the window from a script, without the person's hands.

## Rules

- Probe with a second copy, never the copy the person is using: `ONYX_HOME` at a scratch folder, and `ONYX_VAULT` at a vault made for the probe, never the person's.
- Say what was checked and what was not. A synthetic click does not reach a click handler in a background window, so a picture of the state a click leads to does not show the click works.

## A copy

```
cargo build -p onyx-app
ONYX_HOME=<scratch>/home ONYX_VAULT=<scratch>/vault ONYX_NOTE=<scratch>/vault/Welcome.md ONYX_GO=live ONYX_SHOT=<file.png> ./target/debug/Onyx
```

| Variable | What it does |
|---|---|
| `ONYX_HOME` | the copy's own settings and state |
| `ONYX_APPEARANCE` | `light` or `dark`, for this launch only; never written back |
| `ONYX_LANG` | a language's code (`de`, `zh-Hans`), over the system's |
| `ONYX_VAULT` | the vault to open, over the remembered one; nothing is written to the window's state about it |
| `ONYX_NOTE` | the note to open in it |
| `ONYX_SETTINGS` | open the settings on that section (`appearance`, `text`, `size` for Text's Size page, `about`) |
| `ONYX_GO` | steps, `;` between: `note` (the vault's first note), `notes` (its first three, as tabs), `close` (⌘W: the tab showing, then the window), `hints` (the tabs' numbers, as ⌘ held shows them), `end` (the caret at the note's end, the view with it), `live`, `source`, `reading`, `sidebar`, `menu` (the first note's menu), `rename` (its rename sheet), `vault` (the new-vault sheet; with `HOME` at a scratch folder, ↩ makes the vault in its `Documents`); any value keeps the window over the others |
| `ONYX_SHOT` | a picture of the window (the settings when they are open) after it is drawn, then quit; `ONYX_SHOT_FRAME=1` keeps the system's shadow and rim |
| `ONYX_TRACE` | prints, on stderr, when a key or a click arrives and when the frame after it begins, and what the editor's layout and paint cost, for finding where a wait is; `ONYX_GO=type` types a few words as the input method would |

Keys can be sent to a probe copy with `osascript -e 'tell application "System Events" to keystroke "ab"'` while it is in front, which is how the gap between a key and its frame was measured. The mode is remembered in the copy's home, so a probe that wants Live after one that showed Reading says `live` first. A process may take a picture of its own windows without Screen Recording access, which a terminal running a probe usually lacks.

## The core from a terminal

```
cargo run -p onyx-core -- paths | config | update
cargo run -p onyx-core -- vault <folder>
cargo run -p onyx-core -- vault <folder> new <name>
cargo run -p onyx-core -- --try-update <version> <copy of Onyx.app>
```

`vault <folder>` prints the tree as the window shows it, and how long it took to read, which is how a vault of thousands of notes is timed before anything is made faster.
