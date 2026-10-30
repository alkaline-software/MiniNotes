# The editor

About `crates/onyx-app/src/editor.rs`, `lines.rs` and `markdown.rs`: a note's text, what each line of it is, and the element that lays it out and paints it as it reads, with the marks of whatever the caret is in shown as written.

## Rules

- **The text is `lines::Doc`, a vector of lines, and a place is a line and a byte offset on a character's boundary.** Everything that moves the caret goes through `next_grapheme`, `prev_grapheme`, `word_left`, `word_right` or `clamp`; nothing slices a line at a byte it computed. A slice at a byte inside a character panics, and a panic in a draw takes the app down.
- **Typed text arrives through `EntityInputHandler`** (`replace_text_in_range`, `replace_and_mark_text_in_range`), which gpui calls with the key's character when no binding took it, so an input method composes in place with its candidates window at the caret (`bounds_for_range`). The keys that move, select, delete or format are actions bound in `key_bindings`; Copy, Cut, Paste, Select All, Undo and Redo are the toolkit's own actions, so the Edit menu serves a field and the editor alike. Never bind a printable key.
- **The editor's input handler says it does not accept text input** (`accepts_text_input` is false), so a key goes to the bindings before an input method. On a Mac gpui hands every printable key, ⌥ chords among them, to an active Chinese, Japanese or Korean input method first while the handler says it takes text, and the pinyin method swallowed ⌥1, ⌥2 and ⌥3. A key no binding takes still reaches the method and composes, and on a Mac nothing else reads the flag; it also keeps a multi-stroke binding from waiting on a printable key, and the editor has none. Never bind a printable key, with or without ⌥.
- **Offsets an input method hands over are UTF-16** (`Doc::utf16_of`, `pos_of_utf16`); offsets the markdown reading hands over are bytes of the whole text (`offset_of`, `pos_of`). Mixing the two put the caret inside a character.
- **Several changes at once are one step of undo** (`Doc::replace_many`): a list renumbered, ⌘B around a span, Tab on a selection of lines. Typing within 900 ms of the last keystroke joins its step, erasing likewise; `end_step` parts them where a key should.
- **The whole text is read again after every change** (`markdown::analyze`), and a line is laid out again only when its key changed: its text, its hidden marks, its kind, the width, the faces. A keystroke lays out the lines it touched. A layout computed in `request_layout`'s measure closure is the one `prepaint` and `paint` read, so the three agree.
- **What is hidden is decided at layout, by the caret, never at paint.** The rules are in `Editor::hidden_on` (below). A change to them is a change to the cache key too, or a line keeps its old marks.
- **Paint reads a snapshot** (`PaintInfo`): the cache behind an `Rc`, the selection, the marked range. Painting needs the app the editor is read through, and a read held across a paint call does not compile.
- **A click's place comes from the shown words and is mapped back** (`Shown::to_source`), then snapped past a hidden closing mark, so a press at a rendered span's right edge lands after the whole span and the span stays rendered.

## The text

`lines.rs`. A `Doc` is lines with no line ends; `text()` joins them with `\n`, and a file with Windows' line ends is read as plain ones and written back plain. A `Sel` is an anchor and a head; a caret is the two equal. `replace`, `replace_many`, `undo` and `redo` are the only ways the text changes. `list_line`, `next_marker` and `quote_line` read what a line is for Enter, Tab and ⌘↩.

## What each line is

`markdown.rs`. `analyze` runs pulldown-cmark over the whole text with offsets (tables, strikethrough, task lists, math, wikilinks, YAML headers, footnotes on) and keeps, by line: its kind (`LineKind`: a heading with its level, a fence's open and close, a line of code, a table's first line and its other rows, a rule, a YAML header's edges and lines, a setext underline, HTML, blank), its block marks (a heading's `#`s with the space after, a quote's `>`s), its list mark (the marker as written with its space, its kind, its depth), how many list levels it stands inside, and whether it is a done task. Every inline span is kept whole (`Span`: strong, emphasis, strike, code, link, wikilink, image, highlight, math, footnote, HTML) with its opening and closing marks as byte ranges, in order of start, the outer before the inner, and each line lists the spans that touch it. Tables and code blocks are kept as line ranges, lists as their items' first lines.

`==highlight==` is not the parser's; it is found inside each text run. A wikilink's alias (`[[Note|words]]`) makes the target and the bar part of the opening mark.

## Hidden marks

`Editor::hidden_on`, per line, in Live mode:

| What | Hidden when |
|---|---|
| A heading's `#`s, a quote's `>`s, a setext underline | the selection touches no part of the line |
| A list item's indentation and marker | always: the bullet, the number or the box stands for them |
| An inline span's marks (strong, emphasis, strike, code, highlight, math) | the selection touches no byte of the span, its ends included |
| A link's, wikilink's or image's marks | the selection lies wholly outside the span (a caret at its end does not reveal it, so typing after a link keeps it rendered) |
| A fence's lines | the selection touches no line of the block: the plate keeps its padding where they were, and the language stands at the top right |
| A table's lines after the first | the selection touches no line of the table: the first line draws the grid, the rest have no height |
| A rule's `---` | the selection touches no part of the line: a line is drawn in its place |

**The marks around the caret, and the caret, show only while the keyboard is in the editor** (`Editor::focused`, read at every layout before the rows are keyed): a press in the margin off the words takes the keyboard back to the window (`Shell::render_note`'s scroller, which hears a press the editor did not stop), and the note reads whole again; a tab opened or shown comes up with no caret, the keyboard the window's until a press in the words asks for it (`show_tab`). The caret begins to blink as the keyboard arrives (`kick_blink` at the gain), not at the next edit. Tried and dropped: the marks keyed on the caret alone, which left a block open after a click beside the note and drew a caret that stood still in a note just opened.

In Source mode nothing is hidden and no widget is drawn, but the styles are kept, so a heading is still large, and a code block keeps its plate, its inset and its colors: the source is the note's text shown whole, not the note shown worse. Reading mode is Edit's own drawing with every mark away and nothing to type in (`Editor::locked`, `revealing`): the same layout, spacing and plates as Edit, as the person asked, so the note looks the same whichever way it is shown; a press follows a link or ticks a task and places no caret, the pointer is an arrow, and the keyboard is not taken. A change of mode never moves the keyboard: a caret placed stays placed and comes back with Edit, and a note left by a click in the margin keeps reading whole. Tried and dropped: the toolkit's markdown view for Reading, which laid the note out another way, knew no wikilinks, and brought the caret back at every switch.

## The layout

`Editor::relayout` walks every line, hashes what its layout depends on, and lays out the ones whose hash changed (`lay_out`). A text row is the shown words (`Shown`, the source with the hidden ranges taken out, with the mapping both ways) shaped by gpui at the row's size with a run for each change of face, weight, style, color or decoration, wrapped at the width less the row's inset; its inset is 26 a list level and 18 a quote level. The row's height is its wrapped rows times its line, plus a heading's room above. A row's widgets (the bullet's disc, the number, the task's box, a guide a list level, a bar a quote level) are placed as it is laid out. A table is laid out as a grid: a column as wide as its longest cell wants, scaled down together when the table is wider than the room, and a row as tall as its tallest wrapped cell. The tops are the running sum, and the element's height the total.

A code block's words are colored by the toolkit's tree-sitter highlighter for the fence's language (`color_code`, at every relayout, read again only where the block's text changed), with the inks the theme's `highlight` table names, the same the note gets as it reads; a block with no language or no grammar keeps the plain ink. At the plate's top right stands the language's mark (`langs.rs`: its icon from `icons/lang/`, the set emaki and pingfan.org share, painted in the language's own color, and its name), on the hidden fence's row, which is `MARK_ROOM` taller than the plate's padding for it; a fence shown, with the caret in the block, keeps `FENCE_PAD` of room from the plate's edge. As the note reads the toolkit draws the same mark through `lang_marks` (UPSTREAM.md), its icon in its own colors. Its lines and its fences stand `PLATE_X` in from the plate's edge in every mode, and the plate has `PLATE_PAD` above and below, the room the reading view's plates have, so a note looks the same whichever way it is shown. A blank line in Edit is `BLANK_LINE` of a line, a paragraph's gap as it reads; in Source it is a full line.

Every run's font carries the fallbacks for the characters its face lacks (`Look::text_fallbacks`, `mono_fallbacks`, from `fonts::cjk_fallbacks`): a serif's Chinese behind a serif, a sans serif's behind the rest, so a note set in Source Serif reads in Songti and not in the system's sans. The reading view and the note's title get the same.

The element is one child of the scroller the window draws around it (`Shell::render_note`), so the toolkit's scrollbar and the scroll router serve it; the caret asks for a scroll (`scroll_to_caret`) in `prepaint`, where the rows and the visible bounds are known.

## Paint

Each visible row: the plates behind code blocks first, then a row's chips (inline code's plate, a highlight's wash), the selection's boxes (a wrapped row at a time, and a little past a line's end when the selection runs on), the words, an input method's underline, the widgets, and last the caret, 1.5 wide, blinking at 530 ms on the editor's own timer while it is focused. The pointer: a press puts the caret, a double click takes the word and a triple the line, a drag extends by the grain the press began with; a press on a task's box flips it, and on a rendered link follows it (`EditorEvent::OpenLink`, `OpenNote`), with no caret moved.

## The keys

| Key | Does |
|---|---|
| ↩ | A new line; in a list, the next item with the same marker, the next number or an empty box; on an empty item, the marker goes; in a quote, the next quoted line |
| ⇧↩ | A plain new line |
| ⇥, ⇧⇥ | A list line in or out a level by a tab; the lines of a selection together; outside a list, a tab |
| ⌫, ⌦, ⌥⌫, ⌘⌫ | Back, forward, a word back, to the line's start; ⌫ right after a list marker with nothing typed takes the marker |
| ←→↑↓ and ⇧ | By grapheme; by wrapped row and then by line, keeping the column the first press began in |
| ⌥←, ⌥→ | By word |
| ⌘←, ⌘→, Home, End | The start of the words (past the marks), then the line's start; the line's end |
| ⌘↑, ⌘↓ | The text's start and end |
| ⌘B, ⌘I | Bold and italic: a span of the kind around the caret is unwrapped, a selection or the word under the caret wrapped, an empty pair opened or closed |
| ⌘↩ | A task's box flipped; a bullet made a task; a plain line made a task |
| ⌘Z, ⇧⌘Z, ⌘A, ⌘C, ⌘X, ⌘V | The toolkit's actions |
| ⌘E | Reading and back (the window's) |

Numbered lists count from their first item after every change (`renumber`), one step of undo with the change.

## Not yet

A picture (`![alt](src)`) is drawn as its alt text in the link's color, and opens in the system's viewer at a click. A table's cells are edited as written, with the caret inside the table. There is no find. These are the next things.

## Probing

`ONYX_GO=live;end` opens the note in Live mode with the caret at its end, so the view scrolls there; `source` and `reading` show the other modes (`docs/probing.md`). A synthetic click does not reach the element in a background window, so what a press does is checked by hand in the dev build.
