//! Reading and writing `talkie.md`.
//!
//! The *shape* of the file — where a capture goes, how a save reconciles
//! against what is on disk — lives in `talkie_shared::document`, because the UI
//! has to apply exactly the same rules and two copies would drift. This module
//! is the disk half: paths, atomic writes, and the one function that turns a
//! transcription into a new entry.
//!
//! Captures go at the **top**. Scrolling down walks backwards through time, so
//! the thing you just said is the thing you are looking at.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;
use talkie_shared::document;

/// Local time, to the minute — the heading of one capture.
const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M";

/// Put a capture at the top of the note file, creating the file (and its parent
/// directory) on first use.
///
/// Blank transcriptions are dropped rather than written as an empty heading: a
/// capture that picked up nothing but silence should leave no trace.
pub fn prepend(note_path: &Path, text: &str) -> Result<bool> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(false);
    }

    let existing = read(note_path)?;
    let entry = document::format_entry(text, &Local::now().format(TIMESTAMP_FORMAT).to_string());
    write(note_path, &document::splice(&existing, &entry))?;

    Ok(true)
}

/// Read the note file. A file that does not exist yet reads as empty rather
/// than as an error: the editor opens on a blank document and the first capture
/// (or the first save) creates it.
pub fn read(note_path: &Path) -> Result<String> {
    match fs::read_to_string(note_path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("could not read the notes file {note_path:?}")),
    }
}

/// Replace the note file's contents, atomically.
///
/// Write-then-rename rather than truncate-then-write: the file is shared with
/// Obsidian, with agents watching it, and with `append` above. A reader that
/// arrives mid-save sees either the old file or the new one, never a truncated
/// one. The temporary file is a sibling so the rename stays on one filesystem.
///
/// The trailing newline of the document contract is enforced here too — an
/// editor that strips it would otherwise leave the next appended entry welded
/// to the last line.
pub fn write(note_path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = note_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create the notes folder {parent:?}"))?;
    }

    let text = document::normalized(text);

    let temp = temp_sibling(note_path);
    fs::write(&temp, text.as_bytes())
        .with_context(|| format!("could not write {temp:?} while saving the notes file"))?;
    fs::rename(&temp, note_path).with_context(|| {
        let _ = fs::remove_file(&temp);
        format!("could not replace the notes file {note_path:?}")
    })?;
    Ok(())
}

/// A scratch path next to the real one. The pid keeps two Talkies (a dev build
/// and a bundle, say) from colliding on the same temporary file.
fn temp_sibling(note_path: &Path) -> PathBuf {
    let name = note_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "talkie.md".to_string());
    let temp_name = format!(".{name}.talkie-{}.tmp", std::process::id());
    match note_path.parent() {
        Some(parent) => parent.join(temp_name),
        None => PathBuf::from(temp_name),
    }
}

/// Resolve the configured path, expanding a leading `~`.
pub fn resolve(note_path: &str) -> PathBuf {
    if let Some(rest) = note_path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(note_path)
}

/// Refuse a notes path that a capture could not write to.
///
/// Settings is the one place the path is typed by hand, and a typo there used to
/// save cleanly and fail on the first capture — the shape the accelerator field
/// had in M1. So the checks are the ones a capture would trip over, run now
/// instead of later: the folder is created the way the first capture would
/// create it, and the write is probed the way `write` performs it. Anything
/// that would fail then fails the setting instead.
pub fn validate(note_path: &str) -> Result<(), String> {
    let note_path = note_path.trim();
    if note_path.is_empty() {
        // COPY: settings.note_path.empty — placeholder
        return Err("The notes file needs a path.".to_string());
    }

    let path = resolve(note_path);
    if !path.is_absolute() {
        // COPY: settings.note_path.relative — placeholder
        return Err(format!(
            "`{note_path}` is not a full path — it has to start with / or ~/."
        ));
    }
    if path.is_dir() {
        // COPY: settings.note_path.folder — placeholder
        return Err(format!(
            "`{}` is a folder; the notes file has to be a file inside one.",
            path.display()
        ));
    }

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| format!("`{note_path}` has no folder to live in."))?;
    // COPY: settings.note_path.unwritable — placeholder
    fs::create_dir_all(parent)
        .map_err(|e| format!("Could not create the folder {}: {e}", parent.display()))?;

    // An existing folder is not necessarily a writable one. The probe is the
    // exact write a save makes — a temporary sibling — so what passes here is
    // what will work later.
    let probe = temp_sibling(&path);
    fs::write(&probe, b"")
        .and_then(|()| fs::remove_file(&probe))
        .map_err(|e| format!("Could not write in the folder {}: {e}", parent.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The contract itself — where an entry goes, how a save reconciles — is
    // tested in `talkie_shared::document`. These cover the disk half only.

    fn temp_note(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("talkie-note-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        dir.join("talkie.md")
    }

    #[test]
    fn the_first_capture_creates_the_file_and_its_folder() {
        let path = temp_note("first");
        assert!(prepend(&path, "Hello there").expect("prepend"));

        let contents = read(&path).expect("read");
        assert!(contents.starts_with("## "), "got: {contents:?}");
        assert!(contents.ends_with("Hello there\n"));
    }

    /// The whole point of the change: the newest capture is the one at the top.
    #[test]
    fn the_newest_capture_ends_up_first() {
        let path = temp_note("order");
        prepend(&path, "First said").expect("prepend");
        prepend(&path, "Second said").expect("prepend");

        let contents = read(&path).expect("read");
        let second = contents.find("Second said").expect("second is missing");
        let first = contents.find("First said").expect("first is missing");
        assert!(second < first, "wrong way round: {contents:?}");
        assert!(!contents.contains("\n\n\n"), "too much space: {contents:?}");
    }

    #[test]
    fn a_capture_goes_under_a_title_rather_than_over_it() {
        let path = temp_note("titled");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# talkie.md\n").unwrap();

        prepend(&path, "Spoken").expect("prepend");

        let contents = read(&path).expect("read");
        assert!(
            contents.starts_with("# talkie.md\n\n## "),
            "got: {contents:?}"
        );
    }

    #[test]
    fn blank_transcriptions_are_not_written() {
        let path = temp_note("blank");
        assert!(!prepend(&path, "   \n ").expect("prepend"));
        assert!(!path.exists(), "an empty capture created a file");
    }

    #[test]
    fn a_missing_file_reads_as_empty() {
        let path = temp_note("missing");
        assert_eq!(read(&path).expect("read"), "");
    }

    #[test]
    fn write_then_read_round_trips() {
        let path = temp_note("round-trip");
        write(&path, "## 2026-08-18 09:14\nHello\n").expect("write");
        assert_eq!(read(&path).expect("read"), "## 2026-08-18 09:14\nHello\n");
    }

    #[test]
    fn write_restores_the_trailing_newline() {
        let path = temp_note("trailing");
        write(&path, "no newline here").expect("write");
        assert_eq!(read(&path).expect("read"), "no newline here\n");
    }

    /// An empty document must not gain a newline out of nowhere — that would be
    /// a write where the user made no change.
    #[test]
    fn an_empty_document_stays_empty() {
        let path = temp_note("empty");
        write(&path, "").expect("write");
        assert_eq!(read(&path).expect("read"), "");
    }

    #[test]
    fn validate_accepts_a_path_whose_folder_does_not_exist_yet() {
        let path = temp_note("validate-new");
        assert!(!path.parent().unwrap().exists());
        validate(path.to_str().unwrap()).expect("a creatable folder is fine");
        assert!(
            path.parent().unwrap().is_dir(),
            "the folder was not created"
        );
        assert!(!path.exists(), "validation must not create the file itself");
    }

    #[test]
    fn validate_refuses_an_empty_path() {
        assert!(validate("").is_err());
        assert!(validate("   ").is_err());
    }

    #[test]
    fn validate_refuses_a_relative_path() {
        assert!(validate("talkie.md").is_err());
        assert!(validate("Documents/talkie.md").is_err());
    }

    #[test]
    fn validate_refuses_a_folder() {
        let path = temp_note("validate-folder");
        fs::create_dir_all(&path).unwrap();
        assert!(validate(path.to_str().unwrap()).is_err());
    }

    /// The typo case: a path whose "folder" is in fact a file.
    #[test]
    fn validate_refuses_a_file_in_the_middle_of_the_path() {
        let existing = temp_note("validate-through-file");
        write(&existing, "content").unwrap();
        let through = existing.join("talkie.md");
        assert!(validate(through.to_str().unwrap()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn validate_refuses_a_folder_it_cannot_write_in() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_note("validate-read-only");
        let folder = path.parent().unwrap();
        fs::create_dir_all(folder).unwrap();
        fs::set_permissions(folder, fs::Permissions::from_mode(0o555)).unwrap();

        let result = validate(path.to_str().unwrap());

        // Put it back before asserting, or the next run cannot clear the folder.
        fs::set_permissions(folder, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err(), "a read-only folder passed");
    }

    #[test]
    fn validate_leaves_no_probe_behind() {
        let path = temp_note("validate-no-litter");
        validate(path.to_str().unwrap()).expect("validate");
        let strays: Vec<_> = fs::read_dir(path.parent().unwrap())
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(strays.is_empty(), "left behind: {strays:?}");
    }

    #[test]
    fn write_leaves_no_temporary_file_behind() {
        let path = temp_note("no-litter");
        write(&path, "content").expect("write");

        let parent = path.parent().expect("parent");
        let strays: Vec<_> = fs::read_dir(parent)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(strays.is_empty(), "left behind: {strays:?}");
    }
}
