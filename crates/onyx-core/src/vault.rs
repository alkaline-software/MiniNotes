//! A vault: a folder of markdown notes, as Obsidian has it. Nothing is
//! kept anywhere else: a note is a file, a folder is a folder, and the
//! tree is read off the disk whenever the disk changes (`watch`). A note
//! deleted goes to the system's trash, never straight off the disk.

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::paths;

/// The extensions a note may have. New notes take the first.
pub const NOTE_EXTS: &[&str] = &["md", "markdown"];

/// Folders a vault keeps for itself, not shown in the tree.
const HIDDEN: &[&str] = &[".obsidian", ".onyx", ".trash", ".git"];

/// A folder or a note in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The name as the file manager shows it: a note without its extension.
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
    /// A folder's entries, folders first; empty for a note.
    pub children: Vec<Entry>,
}

pub fn is_note(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| NOTE_EXTS.iter().any(|n| n.eq_ignore_ascii_case(e)))
}

/// A note's name as the tree shows it: the file's stem.
pub fn title(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// The vault's tree: folders first, then notes, each in the order the
/// file manager uses (case folded, a number by its value, so "Note 2"
/// stands before "Note 10"). Hidden entries and the vault's own folders
/// are left out. A folder that cannot be read is empty.
pub fn tree(root: &Path) -> Entry {
    Entry { name: title_of_dir(root), path: root.to_path_buf(), dir: true, children: children(root) }
}

fn title_of_dir(dir: &Path) -> String {
    dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| dir.display().to_string())
}

fn children(dir: &Path) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Entry> = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || HIDDEN.contains(&name.as_str()) {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            out.push(Entry { name, path: path.clone(), dir: true, children: children(&path) });
        } else if is_note(&path) {
            out.push(Entry { name: title(&path), path, dir: false, children: Vec::new() });
        }
    }
    out.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| natural_cmp(&a.name, &b.name)));
    out
}

/// The file manager's order: case folded, a run of digits by its value.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut xn = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    xn.push(c);
                    ai.next();
                }
                let mut yn = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    yn.push(c);
                    bi.next();
                }
                let (xv, yv) = (xn.trim_start_matches('0'), yn.trim_start_matches('0'));
                let by_value = xv.len().cmp(&yv.len()).then_with(|| xv.cmp(yv));
                if by_value != Ordering::Equal {
                    return by_value;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// Every note in the vault, in the tree's order, for a search or a link.
pub fn notes(root: &Path) -> Vec<PathBuf> {
    fn walk(e: &Entry, out: &mut Vec<PathBuf>) {
        for c in &e.children {
            if c.dir {
                walk(c, out);
            } else {
                out.push(c.path.clone());
            }
        }
    }
    let mut out = Vec::new();
    walk(&tree(root), &mut out);
    out
}

pub fn read(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

/// A note written whole: a temp file beside it and a rename, so a reader
/// (another app, a sync) sees the old note or the new one and never half.
pub fn write(path: &Path, text: &str) -> io::Result<()> {
    paths::write_atomic(path, text.as_bytes())
}

/// A name no entry of `dir` has yet: `stem`, then `stem 1`, `stem 2`…
fn free_name(dir: &Path, stem: &str, ext: Option<&str>) -> PathBuf {
    let with = |n: usize| {
        let base = if n == 0 { stem.to_string() } else { format!("{stem} {n}") };
        match ext {
            Some(e) => dir.join(format!("{base}.{e}")),
            None => dir.join(base),
        }
    };
    (0..).map(with).find(|p| !p.exists()).unwrap_or_else(|| with(0))
}

/// A new, empty note in `dir`, named `stem` or the first free name after it.
pub fn new_note(dir: &Path, stem: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = free_name(dir, stem, Some(NOTE_EXTS[0]));
    write(&path, "")?;
    Ok(path)
}

/// A new folder in `dir`, named `name` or the first free name after it.
pub fn new_folder(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let path = free_name(dir, name, None);
    fs::create_dir_all(&path)?;
    Ok(path)
}

/// A new vault: a folder named `name` in `parent`, made along with the
/// folders above it. The name is refused when empty or with a path
/// separator; a folder of that name that is there already becomes the
/// vault as it is, so a folder synced from elsewhere is not refused, but
/// a file of that name is.
pub fn new_vault(parent: &Path, name: &str) -> io::Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a name is one or more characters with no slash"));
    }
    let path = parent.join(name);
    if path.is_file() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a file of that name is there already"));
    }
    fs::create_dir_all(&path)?;
    Ok(path)
}

/// A note or a folder renamed in place. A note keeps its extension; the
/// name is refused when empty, when it has a path separator, or when an
/// entry of that name is there already.
pub fn rename(path: &Path, name: &str) -> io::Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a name is one or more characters with no slash"));
    }
    let parent = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent"))?;
    let target = if path.is_dir() {
        parent.join(name)
    } else {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) if !name.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) => parent.join(format!("{name}.{ext}")),
            _ => parent.join(name),
        }
    };
    if target == path {
        return Ok(target);
    }
    if target.exists() && !same_file_other_case(path, &target) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "an entry of that name is there already"));
    }
    fs::rename(path, &target)?;
    Ok(target)
}

/// On a file system that folds case, "note" and "Note" are one file: a
/// rename between the two is allowed.
fn same_file_other_case(a: &Path, b: &Path) -> bool {
    a.parent() == b.parent() && a.file_name().map(|n| n.to_string_lossy().to_lowercase()) == b.file_name().map(|n| n.to_string_lossy().to_lowercase())
}

/// Put a note or a folder in the system's trash.
pub fn trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// The note a wikilink names, as Obsidian resolves one: `[[Name]]`,
/// `[[folder/Name]]`, with or without the extension, a heading or an
/// alias after `#` or `|` dropped. A bare name is looked for in the
/// link's own folder first, then anywhere in the vault, the shortest
/// path first.
pub fn resolve_link(root: &Path, from: &Path, link: &str) -> Option<PathBuf> {
    let target = link.split(['#', '|']).next().unwrap_or("").trim();
    if target.is_empty() {
        return None;
    }
    let (dir_part, stem) = match target.rsplit_once('/') {
        Some((d, s)) => (Some(d), s),
        None => (None, target),
    };
    let stem = stem.strip_suffix(".md").or_else(|| stem.strip_suffix(".markdown")).unwrap_or(stem);
    let matches = |p: &Path| title(p).eq_ignore_ascii_case(stem);
    if let Some(dir) = dir_part {
        let base = root.join(dir);
        return NOTE_EXTS.iter().map(|e| base.join(format!("{stem}.{e}"))).find(|p| p.is_file());
    }
    let here = from.parent().unwrap_or(root);
    for ext in NOTE_EXTS {
        let p = here.join(format!("{stem}.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    let mut found: Vec<PathBuf> = notes(root).into_iter().filter(|p| matches(p)).collect();
    found.sort_by_key(|p| p.components().count());
    found.into_iter().next()
}

/// The watcher `watch` hands back; it watches for as long as it is kept.
pub type Watcher = notify::RecommendedWatcher;

/// The vault watched: `on_change` is called, on the watcher's own
/// thread, with the paths that changed. The watcher is handed back and
/// watches for as long as it is kept.
pub fn watch(root: &Path, on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> Result<Watcher, String> {
    use notify::Watcher as _;
    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        if let Ok(ev) = res {
            if matches!(ev.kind, notify::EventKind::Access(_)) {
                return;
            }
            on_change(ev.paths);
        }
    })
    .map_err(|e| e.to_string())?;
    watcher.watch(root, notify::RecursiveMode::Recursive).map_err(|e| e.to_string())?;
    Ok(watcher)
}
