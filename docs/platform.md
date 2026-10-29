# Platforms

How Onyx works on macOS, Windows and Linux, each on Intel-class and ARM machines, and what is gated. The studio's constitution has the reasoning; this is what Onyx itself does.

## Rules

- **Everything Unix-only is gated with a stated fallback.** Onyx's own code has no `cfg(unix)` of its own: the vault is `std::fs`, the trash is the `trash` crate (the Finder's, the Recycle Bin, the XDG trash), the watcher the `notify` crate (FSEvents, ReadDirectoryChangesW, inotify). The per-system calls are the template's, in `sys.rs`: open a file or a link, reveal a path (Finder, Explorer, else the folder opened), the window's ground, the languages, a picture of the window for a probe (macOS only; elsewhere it says so).
- **Every child process starts through `onyx_core::process::command`**, so none opens a console window on Windows. Onyx starts none of its own today; the update does.
- **The window's place is saved from `window_bounds()`**, never `bounds()` (`ui_state.rs`).
- **A key is named as the Mac has it and bound as `secondary-<key>`**; off a Mac the caps say Ctrl and Alt.
- **A note's line ends are plain.** A file with Windows' is read as plain ones and written back plain (`lines::Doc::from_text`).

## macOS

The bundle is cargo-packager's with `macos/Info.plist` merged in: the languages, and Markdown as a document type Onyx edits, so Finder offers Onyx for a `.md` file. The dev build is a bundle of its own in `target/debug/dev.noindex/` (`scripts/dev-app.sh`). The installer is a notarized disk image (`docs/release.md`).

## Windows and Linux

As the template has them: Velopack's installer and the Store's `.msix` on Windows, a `.deb`, an AppImage and a snap on Linux, the window's own buttons where the system draws none (`csd.rs`). Neither has been tried on a machine yet for Onyx; the release's step 6 does that in VMs.
