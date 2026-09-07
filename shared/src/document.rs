//! The document contract, as code.
//!
//! `talkie.md` is public API: Obsidian indexes it, agents watch it, and the
//! editor and the capture pipeline both write it. The shape of that file, and
//! the rules for putting something new into it, are defined exactly once — here
//! — because the host and the UI each need to apply them and two copies would
//! drift.
//!
//! ## The shape
//!
//! ```markdown
//! ---
//! tags: [talkie]
//! ---
//!
//! # talkie.md
//!
//! ## 2026-08-18 09:41
//! The newest capture.
//!
//! ## 2026-08-18 09:14
//! An older one.
//! ```
//!
//! **Newest first.** A capture goes at the top, not the bottom, so scrolling
//! down walks backwards through time and the thing you just said is the thing
//! you are looking at. Everything below follows from that one decision.
//!
//! "The top" is not byte zero: YAML frontmatter and a leading `#` title stay
//! where they are. Inserting above frontmatter would silently stop it being
//! frontmatter, which would break the vault the file is sitting in.
//!
//! **One H2 per minute, not per capture.** A capture whose minute matches the
//! heading already at the head of the file joins that entry instead of opening
//! an identical heading above it. Its text goes *below* the text already there,
//! so a same-minute train of thought reads top-down like one paragraph —
//! newest-first still holds between headings.

use serde::{Deserialize, Serialize};

/// Render one entry: an H2 of the timestamp, then the text.
///
/// The caller supplies the timestamp already formatted — this crate compiles to
/// wasm as well as native, and a clock is not something it should own.
pub fn format_entry(text: &str, timestamp: &str) -> String {
    format!("## {timestamp}\n{}\n", text.trim())
}

/// The trailing newline every version of the file ends with.
pub fn normalized(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        return text.to_string();
    }
    format!("{text}\n")
}

/// The byte offset a new entry goes at: after YAML frontmatter and after a
/// leading H1 title, whichever of them are present, and after the blank lines
/// that follow.
///
/// For the ordinary file — one that is nothing but entries — this is 0.
pub fn insertion_offset(text: &str) -> usize {
    let after_frontmatter = frontmatter_end(text);

    // A title line, if the first thing after the frontmatter is one. `# ` and
    // not `##`: an H2 is an entry, and entries are what we are inserting above.
    let mut offset = after_frontmatter;
    let mut cursor = after_frontmatter;
    while cursor < text.len() {
        let line = line_at(text, cursor);
        if line.trim().is_empty() {
            cursor += line.len();
            continue;
        }
        if line.starts_with("# ") {
            offset = cursor + line.len();
        }
        break;
    }

    // Swallow the blank lines after whatever the header turned out to be, so
    // that inserting does not stack another one on top of them.
    while offset < text.len() {
        let line = line_at(text, offset);
        if line.trim().is_empty() {
            offset += line.len();
        } else {
            break;
        }
    }
    offset
}

/// The heading line of the entry at the head of the file — the line at the
/// insertion point, when that line is an H2. Trailing whitespace trimmed.
pub fn head_heading(text: &str) -> Option<&str> {
    let line = line_at(text, insertion_offset(text)).trim_end();
    line.starts_with("## ").then_some(line)
}

/// One past the last non-blank line of the head entry (that line's newline
/// included), or `None` when the file has no head entry. This is where a
/// same-minute capture joins on.
///
/// The entry runs to the next H2 or the end of the file; trailing blank lines
/// are not part of it, so a continuation lands directly under the text rather
/// than below the separator.
pub fn head_block_end(text: &str) -> Option<usize> {
    let start = insertion_offset(text);
    let heading = line_at(text, start);
    if !heading.trim_end().starts_with("## ") {
        return None;
    }

    let mut cursor = start + heading.len();
    let mut end = cursor;
    while cursor < text.len() {
        let line = line_at(text, cursor);
        if line.trim_end().starts_with("## ") {
            break;
        }
        cursor += line.len();
        if !line.trim().is_empty() {
            end = cursor;
        }
    }
    Some(end)
}

/// Where a continuation of the head entry goes, and the exact text to put
/// there: the capture, newline-terminated, with a leading newline when the
/// head block does not already end with one.
pub fn continuation_insert(text: &str, capture: &str) -> Option<(usize, String)> {
    let at = head_block_end(text)?;
    let mut insert = String::new();
    if at > 0 && !text[..at].ends_with('\n') {
        insert.push('\n');
    }
    insert.push_str(capture.trim_end_matches('\n'));
    insert.push('\n');
    Some((at, insert))
}

/// Put a capture into `text`: joined onto the head entry when the minute
/// matches its heading, opened as a new entry otherwise.
///
/// This is the whole "one H2 per minute" rule, and it deliberately reads the
/// file's existing head rather than remembering the last capture — the file
/// changes underneath Talkie between captures.
pub fn splice_capture(text: &str, capture: &str, timestamp: &str) -> String {
    let heading = format!("## {timestamp}");
    if head_heading(text) == Some(heading.as_str()) {
        if let Some((at, insert)) = continuation_insert(text, capture) {
            let mut out = String::with_capacity(text.len() + insert.len());
            out.push_str(&text[..at]);
            out.push_str(&insert);
            out.push_str(&text[at..]);
            return normalized(&out);
        }
    }
    splice(text, &format_entry(capture, timestamp))
}

/// Put `entry` into `text` at the insertion point, with the blank-line
/// separators the contract promises.
pub fn splice(text: &str, entry: &str) -> String {
    let at = insertion_offset(text);
    let (head, tail) = text.split_at(at);

    let mut out = String::new();
    if !head.is_empty() {
        out.push_str(head.trim_end_matches('\n'));
        out.push_str("\n\n");
    }
    out.push_str(entry.trim_end_matches('\n'));
    out.push('\n');
    if !tail.is_empty() {
        out.push('\n');
        out.push_str(tail);
    }
    normalized(&out)
}

/// The two shapes a capture can take in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insertion<'a> {
    /// A whole new entry, heading included, went in at the head.
    Entry(&'a str),
    /// Text joined the head entry — a capture in the same minute. `heading` is
    /// the H2 it went under; a caller re-applying the capture elsewhere needs
    /// it to find (or rebuild) the right entry.
    Continuation { heading: &'a str, text: &'a str },
}

/// What was inserted to turn `base` into `newer` — if an insertion where a
/// capture goes is all that happened.
///
/// This is how a capture is recognised. It is deliberately strict: only the
/// two places the contract puts a capture are tried (the head, and the end of
/// the head entry's text), and everything around the insertion must be
/// untouched, or the change was something else and the caller must not treat
/// it as a capture.
pub fn inserted_at_head<'a>(base: &str, newer: &'a str) -> Option<Insertion<'a>> {
    if newer.len() <= base.len() {
        return None;
    }
    if let Some(inserted) = inserted_at(base, newer, insertion_offset(base)) {
        return Some(Insertion::Entry(inserted));
    }
    let end = head_block_end(base)?;
    let text = inserted_at(base, newer, end)?;
    // The prefix check inside `inserted_at` proved the heading survived, so
    // `newer`'s head heading is the one the text went under — and borrowing it
    // from `newer` keeps this to one lifetime.
    let heading = head_heading(newer)?;
    Some(Insertion::Continuation { heading, text })
}

/// The text spliced into `base` at exactly `at` to produce `newer`, if that is
/// all that separates them.
fn inserted_at<'a>(base: &str, newer: &'a str, at: usize) -> Option<&'a str> {
    let (head, tail) = base.split_at(at);
    if !newer.starts_with(head) || !newer.ends_with(tail) {
        return None;
    }
    let rest = &newer[head.len()..];
    let inserted_len = rest.len().checked_sub(tail.len())?;
    if inserted_len == 0 {
        return None;
    }
    Some(&rest[..inserted_len])
}

/// What a save should do, given three versions of the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Save {
    /// Put this on disk. Either the editor's text, or the editor's text with
    /// someone else's capture put back in at the top.
    Write(String),
    /// The file changed in a way that cannot be reconciled without throwing
    /// something away. The caller refuses, and nothing is lost on either side.
    Conflict,
}

/// Decide what to write.
///
/// - `incoming` — what the editor wants to save.
/// - `on_disk` — what is there right now.
/// - `base` — what the editor last saw, or `None` if it has never read.
///
/// The case that matters: Talkie prepends spoken captures to this file *while
/// the editor may be open with unsaved edits*. Losing one of those to the other
/// is the single worst thing this app could do, so a capture is carried over
/// rather than overwritten, and anything less clear-cut refuses.
pub fn reconcile(incoming: &str, on_disk: &str, base: Option<&str>) -> Save {
    let incoming = normalized(incoming);

    match base {
        // Nothing moved under us: an ordinary save.
        Some(base) if base == on_disk => Save::Write(incoming),
        // A capture arrived. Put it into the editor's text too, wherever that
        // text now says it belongs.
        Some(base) => match inserted_at_head(base, on_disk) {
            Some(insertion) => Save::Write(carry_over(&incoming, insertion)),
            None => Save::Conflict,
        },
        // The editor never read the file, so there is no basis for a merge and
        // nothing to preserve.
        None => Save::Write(incoming),
    }
}

/// Re-apply a capture that landed on disk to the editor's own text.
fn carry_over(text: &str, insertion: Insertion<'_>) -> String {
    match insertion {
        Insertion::Entry(entry) => splice(text, entry),
        Insertion::Continuation {
            heading,
            text: captured,
        } => {
            // Joined onto the same entry when the editor still has it at the
            // head. If the edit moved or renamed that heading, the capture is
            // rebuilt as a full entry instead — text is never dropped just
            // because its heading went away.
            if head_heading(text) == Some(heading) {
                if let Some((at, insert)) = continuation_insert(text, captured) {
                    let mut out = String::with_capacity(text.len() + insert.len());
                    out.push_str(&text[..at]);
                    out.push_str(&insert);
                    out.push_str(&text[at..]);
                    return normalized(&out);
                }
            }
            splice(
                text,
                &format!("{heading}\n{}", captured.trim_end_matches('\n')),
            )
        }
    }
}

/// The end of a YAML frontmatter block, or 0 when the file does not open with
/// one. An unterminated `---` is not frontmatter and is left alone.
fn frontmatter_end(text: &str) -> usize {
    let Some(after_open) = text.strip_prefix("---\n") else {
        return 0;
    };

    let mut offset = "---\n".len();
    let mut cursor = 0;
    while cursor < after_open.len() {
        let line = line_at(after_open, cursor);
        cursor += line.len();
        offset += line.len();
        if line.trim_end() == "---" {
            return offset;
        }
    }
    0
}

/// The line starting at `from`, including its newline.
fn line_at(text: &str, from: usize) -> &str {
    let rest = &text[from..];
    match rest.find('\n') {
        Some(end) => &rest[..=end],
        None => rest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = "## 2026-08-18 09:41\nNewest\n";

    #[test]
    fn the_first_entry_starts_the_file() {
        assert_eq!(splice("", ENTRY), "## 2026-08-18 09:41\nNewest\n");
    }

    #[test]
    fn a_new_entry_goes_above_the_old_ones() {
        let existing = "## 2026-08-18 09:14\nOlder\n";
        assert_eq!(
            splice(existing, ENTRY),
            "## 2026-08-18 09:41\nNewest\n\n## 2026-08-18 09:14\nOlder\n"
        );
    }

    #[test]
    fn a_title_keeps_its_place_at_the_top() {
        let existing = "# talkie.md\n\n## 2026-08-18 09:14\nOlder\n";
        assert_eq!(
            splice(existing, ENTRY),
            "# talkie.md\n\n## 2026-08-18 09:41\nNewest\n\n## 2026-08-18 09:14\nOlder\n"
        );
    }

    /// Inserting above frontmatter would stop it being frontmatter, and take
    /// the Obsidian vault the file lives in with it.
    #[test]
    fn frontmatter_keeps_its_place_at_the_top() {
        let existing = "---\ntags: [talkie]\n---\n\n## 2026-08-18 09:14\nOlder\n";
        assert_eq!(
            splice(existing, ENTRY),
            "---\ntags: [talkie]\n---\n\n## 2026-08-18 09:41\nNewest\n\n## 2026-08-18 09:14\nOlder\n"
        );
    }

    #[test]
    fn frontmatter_and_a_title_together() {
        let existing = "---\ntags: [talkie]\n---\n\n# talkie.md\n\n## 2026-08-18 09:14\nOlder\n";
        let spliced = splice(existing, ENTRY);
        assert!(
            spliced.starts_with("---\ntags: [talkie]\n---\n\n# talkie.md\n\n## 2026-08-18 09:41\n"),
            "got: {spliced:?}"
        );
    }

    /// A lone `---` with no closing delimiter is a horizontal rule, not
    /// frontmatter, and must not swallow the file.
    #[test]
    fn an_unterminated_fence_is_not_frontmatter() {
        assert_eq!(insertion_offset("---\nnot frontmatter\n"), 0);
    }

    #[test]
    fn an_h2_is_not_mistaken_for_a_title() {
        assert_eq!(insertion_offset("## 2026-08-18 09:14\nOlder\n"), 0);
    }

    #[test]
    fn splicing_never_stacks_blank_lines() {
        let existing = "# talkie.md\n\n\n\n## 2026-08-18 09:14\nOlder\n";
        let spliced = splice(existing, ENTRY);
        assert!(!spliced.contains("\n\n\n"), "got: {spliced:?}");
    }

    #[test]
    fn recognises_a_capture_that_arrived_at_the_top() {
        let base = "## 2026-08-18 09:14\nOlder\n";
        let newer = splice(base, ENTRY);
        assert_eq!(
            inserted_at_head(base, &newer),
            Some(Insertion::Entry("## 2026-08-18 09:41\nNewest\n\n"))
        );
    }

    #[test]
    fn a_same_minute_capture_joins_the_head_entry_below_its_text() {
        let existing = "## 2026-08-18 09:41\nFirst thing said\n\n## 2026-08-18 09:14\nOlder\n";
        assert_eq!(
            splice_capture(existing, "Second thing said", "2026-08-18 09:41"),
            "## 2026-08-18 09:41\nFirst thing said\nSecond thing said\n\n## 2026-08-18 09:14\nOlder\n"
        );
    }

    #[test]
    fn a_different_minute_opens_its_own_heading() {
        let existing = "## 2026-08-18 09:14\nOlder\n";
        assert_eq!(
            splice_capture(existing, "Newest", "2026-08-18 09:41"),
            "## 2026-08-18 09:41\nNewest\n\n## 2026-08-18 09:14\nOlder\n"
        );
    }

    #[test]
    fn the_first_capture_of_all_gets_a_heading() {
        assert_eq!(
            splice_capture("", "Newest", "2026-08-18 09:41"),
            "## 2026-08-18 09:41\nNewest\n"
        );
    }

    /// A hand-edited file may end without a newline; joining on must not weld
    /// the two captures into one line.
    #[test]
    fn joining_a_file_without_a_trailing_newline_stays_on_its_own_line() {
        let existing = "## 2026-08-18 09:41\nFirst";
        assert_eq!(
            splice_capture(existing, "Second", "2026-08-18 09:41"),
            "## 2026-08-18 09:41\nFirst\nSecond\n"
        );
    }

    /// The head entry runs to the next H2; blank lines inside it belong to it,
    /// but the trailing separator does not.
    #[test]
    fn a_continuation_lands_under_the_text_not_under_the_separator() {
        let existing = "## 2026-08-18 09:41\nOne\n\nTwo\n\n## 2026-08-18 09:14\nOlder\n";
        assert_eq!(
            splice_capture(existing, "Three", "2026-08-18 09:41"),
            "## 2026-08-18 09:41\nOne\n\nTwo\nThree\n\n## 2026-08-18 09:14\nOlder\n"
        );
    }

    #[test]
    fn recognises_a_capture_that_joined_the_head_entry() {
        let base = "## 2026-08-18 09:41\nFirst\n\n## 2026-08-18 09:14\nOlder\n";
        let newer = splice_capture(base, "Second", "2026-08-18 09:41");
        assert_eq!(
            inserted_at_head(base, &newer),
            Some(Insertion::Continuation {
                heading: "## 2026-08-18 09:41",
                text: "Second\n",
            })
        );
    }

    #[test]
    fn recognises_one_that_arrived_below_a_title() {
        let base = "# talkie.md\n\n## 2026-08-18 09:14\nOlder\n";
        let newer = splice(base, ENTRY);
        assert!(inserted_at_head(base, &newer).is_some());
    }

    #[test]
    fn an_edit_further_down_is_not_a_capture() {
        let base = "## 2026-08-18 09:14\nOlder\n";
        let newer = "## 2026-08-18 09:14\nObsidian rewrote this\n";
        assert_eq!(inserted_at_head(base, newer), None);
    }

    #[test]
    fn an_ordinary_save_writes_what_the_editor_sent() {
        let base = "## 2026-08-18 09:14\nOlder\n";
        assert_eq!(
            reconcile("## 2026-08-18 09:14\nEdited\n", base, Some(base)),
            Save::Write("## 2026-08-18 09:14\nEdited\n".to_string())
        );
    }

    /// The case this function exists for: typing when a capture lands. Both
    /// survive, and the capture ends up on top where it belongs.
    #[test]
    fn a_capture_that_lands_mid_edit_is_carried_over() {
        let base = "## 2026-08-18 09:14\nOlder\n";
        let on_disk = splice(base, ENTRY);
        let incoming = "## 2026-08-18 09:14\nOlder, and edited\n";

        assert_eq!(
            reconcile(incoming, &on_disk, Some(base)),
            Save::Write(
                "## 2026-08-18 09:41\nNewest\n\n## 2026-08-18 09:14\nOlder, and edited\n"
                    .to_string()
            )
        );
    }

    /// Same collision, same-minute shape: the capture joined the head entry on
    /// disk, and the editor was busy editing further down that same entry.
    #[test]
    fn a_continuation_that_lands_mid_edit_is_carried_over() {
        let base = "## 2026-08-18 09:41\nFirst\n\n## 2026-08-18 09:14\nOlder\n";
        let on_disk = splice_capture(base, "Second", "2026-08-18 09:41");
        let incoming = "## 2026-08-18 09:41\nFirst, edited\n\n## 2026-08-18 09:14\nOlder\n";

        assert_eq!(
            reconcile(incoming, &on_disk, Some(base)),
            Save::Write(
                "## 2026-08-18 09:41\nFirst, edited\nSecond\n\n## 2026-08-18 09:14\nOlder\n"
                    .to_string()
            )
        );
    }

    /// If the edit removed the very heading the capture joined, the capture is
    /// rebuilt as a full entry rather than dropped.
    #[test]
    fn a_continuation_whose_heading_was_edited_away_is_rebuilt() {
        let base = "## 2026-08-18 09:41\nFirst\n";
        let on_disk = splice_capture(base, "Second", "2026-08-18 09:41");
        let incoming = "Some prose that replaced the entry\n";

        assert_eq!(
            reconcile(incoming, &on_disk, Some(base)),
            Save::Write(
                "## 2026-08-18 09:41\nSecond\n\nSome prose that replaced the entry\n".to_string()
            )
        );
    }

    #[test]
    fn an_edit_elsewhere_in_the_file_is_a_conflict() {
        let base = "## 2026-08-18 09:14\nold line\n";
        let on_disk = "## 2026-08-18 09:14\nObsidian rewrote this\n";
        assert_eq!(reconcile("mine\n", on_disk, Some(base)), Save::Conflict);
    }

    #[test]
    fn a_first_save_with_no_base_just_writes() {
        assert_eq!(
            reconcile("fresh", "whatever is there", None),
            Save::Write("fresh\n".to_string())
        );
    }

    #[test]
    fn entry_shape_matches_the_contract() {
        assert_eq!(
            format_entry("  Some words  ", "2026-08-18 09:14"),
            "## 2026-08-18 09:14\nSome words\n"
        );
    }
}
