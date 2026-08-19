//! The append engine — and with it, the document contract.
//!
//! Every capture appends exactly:
//!
//! ```markdown
//!
//! ## 2026-08-18 09:14
//! Remember to email Sam about the demo Thursday.
//! ```
//!
//! One H2 per capture, local time, a blank line before each entry, and the file
//! always ends with a newline. This shape is public API: Obsidian indexes it and
//! agents watch it, so changing it breaks other people's setups.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;

/// Local time, to the minute — the heading of one capture.
const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M";

/// Render one entry, without the leading blank line that separates it from
/// whatever came before.
fn format_entry(text: &str, timestamp: &str) -> String {
    format!("## {timestamp}\n{}\n", text.trim())
}

/// Append a capture to the note file, creating the file (and its parent
/// directory) on first use.
///
/// Blank transcriptions are dropped rather than written as an empty heading: a
/// capture that picked up nothing but silence should leave no trace.
pub fn append(note_path: &Path, text: &str) -> Result<bool> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(false);
    }

    if let Some(parent) = note_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create the notes folder {parent:?}"))?;
    }

    let existing = fs::read_to_string(note_path).unwrap_or_default();
    let entry = format_entry(text, &Local::now().format(TIMESTAMP_FORMAT).to_string());

    // Normalise the seam. The contract promises a blank line before every entry
    // and a trailing newline at the end of the file, whatever state the file was
    // left in by an editor, Obsidian, or a crash mid-write.
    let mut prefix = String::new();
    if !existing.is_empty() {
        if !existing.ends_with('\n') {
            prefix.push('\n');
        }
        if !existing.ends_with("\n\n") {
            prefix.push('\n');
        }
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(note_path)
        .with_context(|| format!("could not open the notes file {note_path:?}"))?;
    file.write_all(prefix.as_bytes())?;
    file.write_all(entry.as_bytes())?;
    file.flush()?;

    Ok(true)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &Path) -> String {
        fs::read_to_string(path).expect("read note")
    }

    fn temp_note(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("talkie-note-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        dir.join("talkie.md")
    }

    #[test]
    fn first_entry_has_no_leading_blank_line() {
        let path = temp_note("first");
        append(&path, "Hello there").expect("append");

        let contents = read(&path);
        assert!(contents.starts_with("## "), "got: {contents:?}");
        assert!(contents.ends_with("Hello there\n"));
    }

    #[test]
    fn later_entries_are_separated_by_exactly_one_blank_line() {
        let path = temp_note("separator");
        append(&path, "First").expect("append");
        append(&path, "Second").expect("append");

        let contents = read(&path);
        assert!(contents.contains("First\n\n## "), "got: {contents:?}");
        assert!(!contents.contains("\n\n\n"), "too much space: {contents:?}");
    }

    #[test]
    fn repairs_a_file_that_does_not_end_with_a_newline() {
        // Some other editor left the file mid-line; the contract still has to
        // hold for the entry we add.
        let path = temp_note("no-newline");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# talkie.md").unwrap();

        append(&path, "Entry").expect("append");

        let contents = read(&path);
        assert!(
            contents.starts_with("# talkie.md\n\n## "),
            "got: {contents:?}"
        );
        assert!(contents.ends_with("Entry\n"));
    }

    #[test]
    fn blank_transcriptions_are_not_written() {
        let path = temp_note("blank");
        assert!(!append(&path, "   \n ").expect("append"));
        assert!(!path.exists(), "an empty capture created a file");
    }

    #[test]
    fn entry_shape_matches_the_contract() {
        let entry = format_entry("  Some words  ", "2026-08-18 09:14");
        assert_eq!(entry, "## 2026-08-18 09:14\nSome words\n");
    }
}
