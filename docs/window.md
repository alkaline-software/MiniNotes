# The window

About `crates/onyx-app/src/shell.rs`, `ui_state.rs`, `main.rs`, `settings.rs`, and `crates/onyx-core/src/vault.rs`: the vault, a note's life, the sidebar, the strip, the landing page, the sheets and menus, and what the window remembers.

## Rules

- **A note is written whole** (`vault::write`: a temp file beside it and a rename), 600 ms after the last change (`Shell::note_changed`, by a serial so an older wait does nothing), before another note opens, before the vault closes or changes, at ⌘S, when the window's close button closes it, and at quit (`on_app_quit`). Never in place and never in part: a sync or another app reading the file sees the old note or the new one.
- **A note with changes is never replaced by the disk's text.** The watcher's reading goes into the editor only while `Note::dirty` is false (`disk_changed`), and then quietly (`Editor::replace_text_quietly`), keeping the caret and the history where the text still has them.
- **A note or a folder deleted goes to the system's trash** (`vault::trash`), never `remove_file`.
- **A rename refuses a taken name** and a name with a slash, and keeps a note's extension (`vault::rename`); the open note follows its new path, and so does a note inside a renamed folder.
- **The tree is read off the disk, never kept in step by hand.** After a change of Onyx's own, `reread_tree`; after another app's, the watcher.
- **The vault's folders of its own are not shown** (`.obsidian`, `.onyx`, `.trash`, `.git`, and anything that starts with a dot), and only notes (`.md`, `.markdown`) are listed.
- **Everything the window remembers is written through `UiState`** (`save_ui`), never read from `Config`: the vault and the note are state, not settings.

## The vault

A vault is a folder. `vault::tree` reads it whole, folders first and then notes, each in the file manager's order (case folded, a number by its value, so Note 2 stands before Note 10), and the window keeps the result as `Vault::tree`. The folder is watched (`vault::watch`, the `notify` crate) from the moment it opens; the watcher's thread sends the paths that changed down a channel the window looks at every 300 ms (`watch_changes`), and a burst of them is one reading of the tree off the main thread (`disk_changed`). A note gone from the disk is closed.

Opening a vault (`open_vault`): the open note is written, the tree read, the folded folders of that vault put back from the window's state, the watcher started, and the vault put first among the recent ones (`UiState::remember_vault`, six kept). The system's folder picker is asked through `cx.prompt_for_paths` (`choose_vault`), from the landing page's pill, the sidebar's vault row, File, Open Vault… and ⌘⇧O.

A wikilink names a note by its stem (`vault::resolve_link`): the link's own folder first, then the shortest path anywhere in the vault; a path with a slash is from the vault's root; a heading or an alias after `#` or `|` is dropped. A wikilink to no note makes the note at the vault's root, as Obsidian does (`open_wikilink`). A link that is a path opens the note it names when it is one of the vault's, else the file with the system's app; an address opens in the browser (`open_link`).

## A note's life

`open_note` reads the file, hands the editor its text, remembers it, and focuses the editor. The editor says when the text changed (`EditorEvent::Changed`), and the window writes it as the rules say. A new note (`new_note_in`) is `Untitled.md` or the first free name after it, in the open note's folder, else the vault; it opens and its rename sheet comes up at once, so it gets its name while the person still means it. A new folder likewise. Both come from the sidebar's buttons, File, ⌘N and ⌘⇧N, or a folder's menu.

## The sidebar

`render_sidebar`. The wordmark, the vault's row (its list: Open Vault…, Reveal in Finder, Close Vault), the two buttons, the tree, the foot. The tree is `render_entries`, which flattens the vault's tree into rows and puts a folder's children under its row in a fold that opens and closes in motion (`render_fold`): the children's height is measured by a canvas inside the fold and the box runs up to it and back, the content fading with it, keyed on the folder's path so each change plays once and the first sight plays nothing. The folded set is `Vault::folded`, remembered per vault. A row: the chevron (a folder), Catppuccin's icon, the name, on a plate while it is the open note. A right click opens the row's menu (`entry_menu`): a note's is Open, Reveal, Copy Path, Rename…, Move to Trash; a folder's is New Note, New Folder, Reveal, Rename…, Move to Trash.

## The strip and the modes

The strip is the card's top: the sidebar's button at the left over the traffic lights, drawn once so it does not move with the sidebar; the note's name in the middle with a dot while changes wait; at the right the modes as a segmented control (Edit, Source, Read, with a pencil, a file of code and an open book) and the gear. The mode is the editor's (`Editor::set_mode`) and is remembered (`UiState::mode`), so a note opens as the last one was shown. ⌘E goes to Read and back; the View menu names all three.

## The landing page

With no vault: the name in the name's face and the accent, one sentence, the primary pill, and the recent vaults on a card. With a vault and no note: one line and a New Note pill. Each page fades and rises in (`page_in`) when it arrives.

## The rename sheet

`render_rename`: a card over a scrim, the name in the toolkit's `Input` (`rename_input`, made with the window, since an input watches the window it was made in), ↩ keeps it (the input's `PressEnter`), Esc or a press off the card lets it go. The sheet occludes, so a press in it let go outside does not close it. After it the keyboard goes back to the editor when a note is open, else to the window.

## The settings

`settings.rs`, the studio's window: a rail of three sections, the page at the right, drawn from the main view's state so a change shows in both windows. Appearance has the appearance and the accent, and the language at the right of its title. Editor has the words' face and code's face as tiles (`tiles`), the text size, and Readable line length. About has the title card and the updates. A change to a face or the size reaches the editor through `Shell::push_look`.

## What the window remembers

`ui_state.rs`, `~/.onyx/state/ui.json`: the bounds (from `window_bounds()`, never `bounds()`), whether the sidebar is open, the settings' section, the vault, the note, the mode, the recent vaults, the folded folders. Written when any of it changes (the bounds at most every two seconds), at quit, and when the close button closes the window.

## Probing

`ONYX_VAULT=<folder>` opens that vault instead of the remembered one, `ONYX_NOTE=<file>` that note; `ONYX_GO` steps: `note` (the first note), `sidebar`, `menu` (the first note's menu), `rename` (its rename sheet), `live`, `source`, `reading`, `end`. `ONYX_SETTINGS=editor` opens the settings on that section (`docs/probing.md`).
