//! The core's tests. Each points `ONYX_HOME` at a temp folder of its
//! own (`isolated`), so none reads or writes the person's files.

use std::sync::Mutex;

use onyx_core::{config::Config, paths, update};

/// Tests run on threads of one process and share its environment.
static ENV: Mutex<()> = Mutex::new(());

fn isolated() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("a temp folder");
    std::env::set_var("ONYX_HOME", dir.path());
    std::env::remove_var("ONYX_APPEARANCE");
    (dir, guard)
}

#[test]
fn a_missing_or_broken_file_is_the_defaults() {
    let (_dir, _g) = isolated();
    assert_eq!(Config::load(), Config::default());
    std::fs::write(paths::config_file(), "{ not json").unwrap();
    assert_eq!(Config::load(), Config::default());
}

#[test]
fn a_file_with_a_field_missing_still_loads() {
    let (_dir, _g) = isolated();
    std::fs::write(paths::config_file(), r#"{"app": {"accent": "jade"}}"#).unwrap();
    let cfg = Config::load();
    assert_eq!(cfg.app.accent, "jade");
    assert_eq!(cfg.app.appearance, "system");
}

#[test]
fn an_override_is_never_written_back() {
    let (_dir, _g) = isolated();
    std::env::set_var("ONYX_APPEARANCE", "dark");
    assert_eq!(Config::load().app.appearance, "dark");
    Config::edit(|c| c.app.accent = "rose".into()).unwrap();
    std::env::remove_var("ONYX_APPEARANCE");
    let cfg = Config::load();
    assert_eq!(cfg.app.appearance, "system");
    assert_eq!(cfg.app.accent, "rose");
}

#[test]
fn a_write_leaves_no_temp_file() {
    let (dir, _g) = isolated();
    paths::write_json(&paths::state_dir().join("x.json"), &serde_json::json!({"a": 1})).unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path().join("state")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["x.json"]);
}

#[test]
fn the_version_is_read_off_the_redirect() {
    assert_eq!(update::version_from_location("https://github.com/o/r/releases/tag/v1.2.3").as_deref(), Some("1.2.3"));
    assert_eq!(update::version_from_location("https://github.com/o/r/releases/tag/1.2.3/").as_deref(), Some("1.2.3"));
    assert_eq!(update::version_from_location("https://github.com/o/r/releases"), None);
    assert!(update::is_newer("0.10.0", "0.9.9"));
    assert!(!update::is_newer("1.0", "1.0.0"));
}

#[test]
fn every_system_has_an_installer_name() {
    let name = update::asset_name().expect("this system has an installer");
    assert!(name.starts_with(update::APP), "{name}");
}

// -- the vault ---------------------------------------------------------------

use onyx_core::vault;

fn a_vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp folder");
    let r = dir.path();
    std::fs::create_dir_all(r.join("Projects/Onyx")).unwrap();
    std::fs::create_dir_all(r.join(".obsidian")).unwrap();
    std::fs::write(r.join("Note 10.md"), "# ten").unwrap();
    std::fs::write(r.join("Note 2.md"), "# two").unwrap();
    std::fs::write(r.join("about.md"), "").unwrap();
    std::fs::write(r.join("picture.png"), "").unwrap();
    std::fs::write(r.join(".hidden.md"), "").unwrap();
    std::fs::write(r.join("Projects/Onyx/Plan.md"), "").unwrap();
    std::fs::write(r.join("Projects/Plan.md"), "").unwrap();
    dir
}

#[test]
fn the_tree_has_folders_first_in_the_file_managers_order() {
    let dir = a_vault();
    let tree = vault::tree(dir.path());
    let names: Vec<String> = tree.children.iter().map(|e| format!("{}{}", e.name, if e.dir { "/" } else { "" })).collect();
    assert_eq!(names, ["Projects/", "about", "Note 2", "Note 10"]);
    let projects = &tree.children[0];
    assert_eq!(projects.children.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Onyx", "Plan"]);
    assert_eq!(vault::notes(dir.path()).len(), 5);
}

#[test]
fn a_new_note_takes_the_first_free_name() {
    let dir = a_vault();
    let a = vault::new_note(dir.path(), "Untitled").unwrap();
    let b = vault::new_note(dir.path(), "Untitled").unwrap();
    assert_eq!(a.file_name().unwrap(), "Untitled.md");
    assert_eq!(b.file_name().unwrap(), "Untitled 1.md");
    assert_eq!(vault::new_folder(dir.path(), "Projects").unwrap().file_name().unwrap(), "Projects 1");
}

#[test]
fn a_rename_keeps_the_extension_and_refuses_a_taken_name() {
    let dir = a_vault();
    let renamed = vault::rename(&dir.path().join("about.md"), "About Onyx").unwrap();
    assert_eq!(renamed.file_name().unwrap(), "About Onyx.md");
    assert!(renamed.is_file());
    assert!(vault::rename(&renamed, "Note 2").is_err());
    assert!(vault::rename(&renamed, "").is_err());
    assert!(vault::rename(&renamed, "a/b").is_err());
    let folder = vault::rename(&dir.path().join("Projects"), "Work").unwrap();
    assert!(folder.join("Plan.md").is_file());
}

#[test]
fn a_wikilink_finds_the_nearest_note_of_that_name() {
    let dir = a_vault();
    let r = dir.path();
    let from = r.join("Projects/Onyx/Plan.md");
    assert_eq!(vault::resolve_link(r, &from, "Plan"), Some(r.join("Projects/Onyx/Plan.md")));
    assert_eq!(vault::resolve_link(r, &r.join("about.md"), "plan#Heading|alias"), Some(r.join("Projects/Plan.md")));
    assert_eq!(vault::resolve_link(r, &from, "Projects/Plan.md"), Some(r.join("Projects/Plan.md")));
    assert_eq!(vault::resolve_link(r, &from, "Nowhere"), None);
}

#[test]
fn a_note_is_written_whole_and_leaves_no_temp_file() {
    let dir = a_vault();
    let note = dir.path().join("about.md");
    vault::write(&note, "words").unwrap();
    assert_eq!(vault::read(&note).unwrap(), "words");
    let names: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".tmp")).collect();
    assert!(names.is_empty(), "{names:?}");
}

#[test]
fn names_sort_as_the_file_manager_sorts_them() {
    use std::cmp::Ordering::*;
    assert_eq!(vault::natural_cmp("Note 2", "Note 10"), Less);
    assert_eq!(vault::natural_cmp("b", "A"), Greater);
    assert_eq!(vault::natural_cmp("x", "x"), Equal);
    assert_eq!(vault::natural_cmp("note", "note 1"), Less);
}
