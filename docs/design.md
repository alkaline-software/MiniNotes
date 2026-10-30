# Design

The standard every part of Onyx a person sees is held to: the window, the sidebar, the note, the settings, the icon. The bar is the work that wins Awwwards, the Webbys and the FWA: minimal, calm, intuitive, finished in every state. The studio's constitution is where this comes from; this file is Onyx's own, in its terms, and is its standard. How the window's parts work is in `docs/window.md`, the editor in `docs/editor.md`.

## Rules

- **Calm.** One accent, the person's choice of seven, for what is on, in use, chosen, or the one thing a page is for: a link, inline code's plate, a quote's bar, a done task's box, the chosen tile. Nothing else competes with it.
- **Everything fits.** Every section of the settings shows all it has without scrolling. The page still scrolls for a window made smaller.
- **Nothing jumps.** What appears, goes, opens, closes, grows, moves or turns does so over a moment, with the pattern its neighbors use (`docs/window.md`, Motion). No pause at the end of a motion.
- **What is cut says so.** A list that runs past an edge fades out at that edge.
- **A gesture stays where it began.** A scroll moves only the scroller it started in (`scroll.rs`).
- **Words are few.** A row's name says what it is. The window's chrome says nothing the note does not need.
- **One way to draw each thing.** A setting is a row on a card; on or off is a switch; a choice among a few is a segmented control; a button is a pill with an icon before its word; a face is a tile drawn in itself; a key is a cap. A new control is a new pattern for every page, or it is not made.
- **Both appearances, every state.** Light and dark are designed together. A change is looked at in both, and in every state it has: no vault, a vault with no note, a note, each mode, a menu, the rename sheet, a narrow window.
- **Readable.** Words read on their ground at 4.5 to 1 or better. A key is drawn as a key.
- **Look before saying it is done** (`docs/probing.md`).

## The palette

The neutrals are the studio's, in `themes/onyx.json`: the page `#FBFBFA` and `#1A1918`, the ink `#1F1E1D` and `#F3F1EA`, the muted `#77746B` and `#A09D94`, the panel `#F0EFED` and `#222120`, the border `#DFDFDC` and `#32312E`, the frame a warm graphite `#3D3A36` and `#3A3733`. The frame is the sidebar's ground in both appearances, so the note's page is a well in a lighter shell. Code's inks are the studio's Rosé Pine set (`look::Inks`); they color the reading view's code blocks.

### The accent

Onyx offers the seven accents (`look::ACCENTS`, chosen in Settings, Appearance) and wears **teal** on a new install: `#277580` in the light appearance, `#6FB9C4` in the dark. Onyx is the black stone, and the studio's navy is the black that is not black; of the seven, teal is the coolest and the farthest from Obsidian's purple, and on the navy it reads as the sheen on polished stone. Periwinkle is the studio's own default and jade reads as green onyx, another stone. The icon's band of light is teal's dark shade whatever accent is worn.

## The type

- **The chrome** is the system's face: the sidebar, the strip, the settings, the sheets.
- **A note's words** are in a face of the person's choice (Settings, Text, Fonts), the twelve emaki offers under the same keys (`fonts.rs`): four serifs (Source Serif 4, Newsreader, Literata, Lora) and four sans serifs (Inter to begin with, IBM Plex Sans, Source Sans 3, Geist), each carried as a static file a weight under the SIL Open Font License. None of them has a Chinese, Japanese or Korean character, so behind each stands a carried Chinese face (`fonts::CJK`, Settings, Text, Fonts, Chinese font): Noto Serif SC and Noto Sans SC, the simplified Chinese subsets of the Source Han faces, regular and bold, and LXGW WenKai, a kai hand, regular and medium; Auto, the default, is Noto Serif SC behind a serif face and Noto Sans SC behind a sans serif or a monospace one (`fonts::cjk_fallbacks`). Behind the carried face stand the system's faces of that kind for Japanese, Korean and the rest (Songti SC or PingFang SC, Hiragino, Apple SD Gothic Neo), and the system's emoji last. The six Chinese files are most of the bundle's fonts, about 90 MB in all, which the person chose over the system's faces so a note looks the same on every Mac. Under each font card a field names a font installed on the machine instead (`EditorConfig::text_custom`, `mono_custom`, `cjk_custom`), as Obsidian offers; while it has a name the tiles light none, and a tile chosen clears it. The note's name over its words is in that face too.
- **Code** is in a monospace face of the person's choice: JetBrains Mono to begin with, Monaco from the system on a Mac (IBM Plex Mono, carried, elsewhere), Maple Mono or Geist Mono. Its size is its own (`TextSizes::code`), not a share of the words'.
- **The name** Onyx, in the sidebar's wordmark, the landing page and About, is Optima where the system has it, else Belleza, carried. Whether the system has it is asked once (`fonts::wordmark_family`): listing every installed face costs about a tenth of a second on a Mac with many fonts, and asked at every frame it held every keystroke and click that long.
- **Sizes.** The words are 14, 16 or 18 points (Small, Medium, Large), on a line of 1.6. Headings go by Obsidian's scale of the words' size: 1.618, 1.462, 1.318, 1.188, 1.076 and 1, the first bold and the rest semibold, on lines of 1.25 to 1.5, with a third of an em over a heading that does not follow a blank line. Code is 0.9 of the words' size on a line of 1.5. The note's name over its words is 1.8 times bold.
- The chrome's sizes are the studio's: a section's name 15 semibold, a row's name 13, a control's text 12.5, a group's name 12 semibold in the muted ink, a row's line 11.5 on 15.

## Icons in the window

Tabler's outline icons at a stroke of 1.75, under the names the toolkit asks for, in `assets/icons/`; the tree's icons are Catppuccin's (`file_icons.rs`), Latte in the light appearance and Mocha in the dark, a note wearing Markdown's. The strip's modes wear a pencil, a file of code and an open book. Every button has an icon before its word; a choice in a menu has one too. A key is drawn on a cap (`widgets::key_text`).

## The app icon

A note on a stone: a broad oval onyx laid down, in the studio's navy with a straight band of light in teal's dark shade, and a square note set askew on it, so the stone shows at the note's edges; three lines of writing, the amber one in the middle, and the violet spark at the top right, on the studio's cream washed with a fifth of teal (`#e1e8e2` to `#c9d2c5`: the cream gradient's ends mixed with the mean of teal's two base shades, `#277580` and `#6FB9C4`, 16% at the top left and 24% at the bottom right), so the Dock tells Onyx from the studio's other icons by its ground while the drawing stays in the studio's hand. The constitution names the mix; five strengths were tried, and a fifth was the one where the ground read as the app's own without losing the cream. The note is the note-taker's thing; the stone it rests on makes it Onyx, and read as one thing where a stone beside the note read as an add-on.

`scripts/icon/icon.html` draws it, with ten candidates of the note on the stone under `CANDIDATES`, each drawn on its own pixels at every size and proofed at 32 and 16; `VARIANT` names the one in use and `scripts/make-icon.sh` renders every size into `assets/icon/`. Below 32 pixels the three lines become two thicker ones and the band grows a little. The first candidate is the one in use; the other nine vary the stone's shape and tilt, the note's size and corners, and where the two sit, and each lost to it for showing less of the stone's band or less of the note. Tried and dropped before them: the stone beside the note (an add-on), a stone of straight edges (the five-sided one hard to look at), and a faceted gem (too many angles).

## The window

Laid out as the studio's windows are: the frame is the window's ground, the sidebar stands on it with no edge of its own, and the note is a card with rounded corners (12 on a Mac) on a band of the frame 4 wide. The traffic lights and the sidebar's button stand on one line and do not move whether the sidebar is there or not. The window opens at 1080 by 720 and is never narrower than 720.

- **The sidebar**, 240 wide: the wordmark, the vault's name on a row whose list opens, reveals or closes the vault, two small buttons for a new note and a new folder, then the tree, and at the foot the key that hides the sidebar. A folder's row has a chevron and folds in motion; a note's row wears the Markdown icon; the open note's row stands on a plate. A right click on a row opens its menu.
- **The strip** over the note: the note's name in the middle in the muted ink, a dot beside it while changes wait to be written, and at the right the three modes as a segmented control and the settings' gear.
- **The note** runs in a column 700 wide, centered, while Readable line length is on, else the card's width, with its name over it in the words' face. Its scroller fades at the top while there is more above.
- **With no vault** the card is the landing page: the name, one sentence, Open a vault… as the one primary pill, and the vaults opened before on a card, each with its folder's path under its name. With a vault and no note, one line and a New Note pill.
- **A rename** is a sheet over the window: a card with the name in a field, Cancel and Rename, ↩ and Esc on caps under it, on a scrim any press puts away.
- **A notice** (a note that could not be written, a name not changed) is one line in the danger's ink under the strip, gone after a few seconds.

### The settings

The studio's settings window, 820 by 600, three sections: **Appearance** (the appearance, the accent, and the language at the right of the title), **Editor** (the words' face and code's face as tiles drawn in themselves, the text size as a segmented control, Readable line length as a switch) and **About** (the title card, the credits, the updates). Each fits without scrolling.

## Motion

The studio's patterns and timings (`widgets.rs`): a menu in over 140 ms and out over 110, a fold and a switch over 200, a segmented control's plate and the sidebar over 220, a page's content fading and rising in over 240, every one eased with `ease_out_quint`. In Onyx: the sidebar slides in from the window's edge as its room opens; a folder's children fold open under its row and close back to it, the first sight playing nothing; the mode's plate slides between the three; a page (the landing page, a note) fades and rises in when it arrives, keyed on the note; a menu comes in rising and goes out fading where it was; the rename sheet fades in over its scrim; a notice is drawn while it lasts. Left as they are on purpose: the editor's own text as it is typed, the caret, and a scroll the caret asks for, which is at once so the caret is never out of sight.

## Checking

Shoot every state a change touches, in both appearances and in a long language, and look at it whole and close up (`docs/probing.md`). Then go round again: what is out of line, louder than it needs to be, or not consistent with its neighbors is fixed and shot again, until nothing is left to change.
