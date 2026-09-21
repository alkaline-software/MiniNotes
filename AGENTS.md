# AGENTS.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

MiniNotes is a macOS menu bar app (no Dock icon; `LSUIElement = true`) with a live-preview Markdown editor. The UI is a SwiftUI/AppKit popover wrapping a WKWebView that runs a CodeMirror 6 editor bundled from JavaScript.

## Current Version

**v0.4.1** — update `MARKETING_VERSION` and `CURRENT_PROJECT_VERSION` in `project.yml` (both must match) when releasing a new version, then run `xcodegen generate`.

## Build Commands

### JS Editor Bundle (required after any change to `build/src/editor.js` or `editor.html`)

```bash
cd build && npm run build
```

This bundles and minifies `build/src/editor.js` into `MiniNotes/Resources/editor-bundle.js` using esbuild. The Swift app reads this file at runtime and inlines it into the HTML.

### macOS App (CLI build + relaunch — preferred; do this after every change session)

```bash
xcodebuild -project MiniNotes.xcodeproj -scheme MiniNotes -configuration Release build
killall MiniNotes 2>/dev/null; sleep 1
ditto ~/Library/Developer/Xcode/DerivedData/MiniNotes-dqmwdyrthfgyezeddoegrwdqawje/Build/Products/Release/MiniNotes.app /Applications/MiniNotes.app
rm -rf ~/Library/Developer/Xcode/DerivedData/MiniNotes-dqmwdyrthfgyezeddoegrwdqawje/Build/Products/Release/MiniNotes.app
open /Applications/MiniNotes.app
```

The user does not use Xcode for day-to-day testing — always rebuild and relaunch the app this way when a session's changes are done. Install to `/Applications` and delete the DerivedData copy so the app never appears twice in Spotlight; the next build recreates the DerivedData app.

### Regenerate Xcode project (required after any change to `project.yml`)

```bash
xcodegen generate
```

## Architecture

### Startup flow

`MiniNotesApp` (SwiftUI `@main`) -> `AppDelegate` -> `StatusBarController`

`StatusBarController` owns:
- `NSStatusItem` (menu bar icon: custom-drawn template badge — outlined 22x16pt rounded rect with a solid pen, inverting to a filled badge while the popover is open — matching the system input-method icon; drawn in `makeStatusIcon(active:)`; `autosaveName` "MiniNotes")
- `NSPopover` (620x500, `.applicationDefined` behavior)
- `NotesStore` (single source of truth for note content and app state)
- `EventMonitor` (closes popover on outside clicks)
- Global hotkey registration via `HotkeyManager` (default ⌃⌥⌘N, customizable in Settings)

**Status item self-healing**: menu bar managers (Ice, Hidden Bar) hide items by pushing them off-screen, and macOS persists that position in the app's own defaults (`NSStatusItem Preferred Position MiniNotes`), so the icon can stay invisible across relaunches. `healStatusItemIfNeeded()` detects an off-screen button window and recreates the item at a known-visible position. It runs 2s after launch, on app reopen, and before every popover open.

`AppDelegate` also owns the Sparkle `SPUStandardUpdaterController` (auto-update; see Release section).

### View state machine

`ContentView` switches between two root views based on `NotesStore.isFileOpen`:

- **`LandingView`** — shown on fresh start (no previously stored file) or after clicking the exit button. Lets the user open an existing `.md` file, create a new one, or reopen the most recently closed file.
- **Editor view** — toolbar (`pin | filename … [source|edit|view] … Open in Obsidian | exit`) + `MarkdownEditorView` (WKWebView). Editor modes switch with Cmd+1/2/3. "Open in Obsidian" launches `obsidian://open?path=` on the same file on disk (no copying; the file must live inside a vault Obsidian knows, though not necessarily the currently open one).

Clicking the exit button calls `NotesStore.closeFile()`: flushes to disk, records `recentFileURL`, sets `isFileOpen = false`.
Picking a file from `LandingView` calls `NotesStore.changeFile(to:)`: loads content, sets `isFileOpen = true`.

### Editor bridge (Swift <-> JavaScript)

The editor is a `WKWebView` rendering `editor.html` with `editor-bundle.js` inlined. Communication is bidirectional:

- **JS -> Swift**: `window.webkit.messageHandlers.contentChanged.postMessage(text)` triggers `NotesStore.save(_:)`
- **JS -> Swift**: `window.webkit.messageHandlers.openURL.postMessage(url)` opens links via `NSWorkspace`
- **Swift -> JS**: `webView.evaluateJavaScript(...)` calls these global functions defined in `editor.js`:
  - `setInitialContent(content)` — called once after page load
  - `resumeEditor(content)` — called when popover opens; recreates editor if it was destroyed
  - `suspendEditor()` — called when popover closes; destroys the CodeMirror view to free resources

### Persistence

`NotesStore` saves the file on popover close (`flush()`) and on exit button click (`closeFile()`). The last-used file path is stored in `UserDefaults` under key `notesFilePath`. On first launch (no stored path), `LandingView` is shown instead of the editor.

### JavaScript editor (`build/src/editor.js`)

Built on CodeMirror 6. Key features:
- **`livePreviewPlugin`**: walks the syntax tree on every doc/selection change, hides Markdown syntax markers (heading `#`, bold `**`, italic `*`, links) when the cursor is not on that line/range, and applies CSS classes for styled rendering.
- **`emptyLineSelectionPlugin`**: detects empty lines within the current selection and adds class `lp-empty-in-sel`, which shows a thin left-edge highlight strip (native browser selection skips empty lines).
- **Tables**: rendered as interactive HTML widgets via `TableWidget` (`WidgetType`); cells are editable `contenteditable` divs.
- **Checkboxes**: `- [ ]` / `- [x]` task list items rendered as clickable checkbox widgets.
- **Ordered list auto-renumber**: `renumberOrderedLists` keeps list numbers correct on every edit.
- **Formatting hotkeys** (`build/src/formatting.js`): ⌘B/⌘I smart-toggle bold `**` and italic `*`. Syntax-tree based, so toggling works with the caret anywhere inside a styled span, with or without the markers selected, and through nesting like `***bold italic***`. Headless test suite: `cd build && npm test`.
- **Nested lists** (`build/src/lists.js`): Tab/Shift-Tab indent/outdent list items (bullet, ordered, task) one level by inserting/removing a leading tab, Obsidian-style — the caret can be anywhere on the line, and range selections indent every list line they touch. Outside lists, Tab inserts a literal tab. Nested ordered lists renumber independently per level (`orderedListRenumberChanges`, called by `renumberOrderedLists`). Leading indent units get class `lp-indent-guide`, which draws Obsidian-style vertical indent guides. Tests in `build/test/lists.test.mjs`.
- **Edge-click snap + touch reveal** (`edgeClickSnap` transaction filter, `cursorTouchesRange`): clicking the left/right edge of a rendered styled span (bold/italic/strikethrough/inline code) places the caret outside the whole construct (before/after the markers), and a caret touching either end of such a span reveals its raw source — Obsidian-style. Clicking mid-span reveals with the caret inside.
- **`drawSelection` is used** — custom cursor/selection rendering eliminates WebKit ghost-caret artifacts. `emptyLineSelectionPlugin` complements it by highlighting empty lines inside selections (which `drawSelection` skips).

CSS classes applied by the plugin: `lp-h1`–`lp-h6`, `lp-strong`, `lp-em`, `lp-code`, `lp-fenced-line`, `lp-link`, `lp-bullet`, `lp-bullet-line`, `lp-ordered-line`, `lp-checkbox`, `lp-checkbox-checked`, `lp-empty-in-sel`, `lp-indent-guide`.

Decoration ordering rules: line decorations before mark decorations; overlapping `Decoration.replace` items are deduplicated.

## Key Files

| File | Purpose |
|------|---------|
| `project.yml` | XcodeGen spec — edit versions here, then run `xcodegen generate` |
| `MiniNotes/App/AppDelegate.swift` | App entry, delegates to `StatusBarController` |
| `MiniNotes/StatusBar/StatusBarController.swift` | Menu bar icon, popover lifecycle, event monitor |
| `MiniNotes/Store/NotesStore.swift` | `isFileOpen`, `recentFileURL`, content, debounced persistence |
| `MiniNotes/Views/ContentView.swift` | Root view: switches between `LandingView` and editor |
| `MiniNotes/Views/LandingView.swift` | Landing page: file picker, reopen recent, author info |
| `MiniNotes/Views/MarkdownEditorView.swift` | `NSViewRepresentable` wrapping WKWebView; JS bridge |
| `MiniNotes/Views/SettingsView.swift` | File-location settings sheet (opened from toolbar filename button) |
| `MiniNotes/Views/AppSettingsView.swift` | App Settings window (right-click menu): language, hotkey, auto-update, About |
| `MiniNotes/Store/AppSettings.swift` | User preferences (language, hotkey, auto-update toggle), holds Sparkle updater ref |
| `MiniNotes/Helpers/HotkeyManager.swift` | Carbon global hotkey registration |
| `MiniNotes/Helpers/LocalizationHelper.swift` | All UI strings (EN/ZH), notification name extensions |
| `appcast.xml` | Sparkle update feed, served from raw.githubusercontent.com on `main` |
| `scripts/create-dmg.sh` | Signed DMG with custom background and Finder icon layout |
| `MiniNotes/Assets.xcassets/` | App icon (notebook + pencil, lavender bg) at all required sizes |
| `MiniNotes/Resources/editor.html` | HTML shell; JS bundle is inlined at load time |
| `build/src/editor.js` | CodeMirror 6 editor + live-preview decorations |
| `build/src/formatting.js` | ⌘B/⌘I smart formatting toggles (tests in `build/test/`) |
| `build/src/lists.js` | Tab/Shift-Tab list indent/outdent, nested ordered-list renumbering (tests in `build/test/`) |
| `build/package.json` | JS dependencies and esbuild script |

## Typography

All UI text uses two fonts (assumed installed in `~/Library/Fonts/`):
- **LXGW WenKai Medium** (`LXGWWenKai-Medium`) — all body/UI text
- **Maple Mono NF CN Regular** (`MapleMono-NF-CN-Regular`) — monospace/code contexts, version string

## Release

Use the `/ph-app` skill (`~/.claude/skills/ph-app`, `resources/release.md`) for the full workflow: version bump, Release build, Developer ID signing, notarization, DMG/ZIP packaging, Sparkle signing, appcast update, and GitHub publish.

Auto-update is Sparkle (SPM, `SUFeedURL` and `SUPublicEDKey` in `project.yml`). Release DMGs must be signed with the EdDSA key matching the shipped `SUPublicEDKey` — the private key lives in the login keychain ("Private key for signing Sparkle updates") with a backup in iCloud Passwords ("Sparkle EdDSA Private Key"). Verify with Sparkle's `generate_keys -p` before releasing on a new machine.
