//! The document contract, as code.
//!
//! `talkie.md` is public API: Obsidian indexes it, agents watch it, and the
//! editor and the capture pipeline both write it. This defines all interactions with that file, on both the host and UI sides.
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
//! An earlier capture from the same minute.
//!
//! ## 2026-08-18 09:14
//! An older one.
//! ```
//!
//! **Newest first.** A capture goes at the top, not the bottom, so scrolling
//! down walks backwards through time and the thing you just said is the thing
//! you are looking at.
//!
//! **One heading per minute.** A capture made in the minute the top heading
//! already names goes directly under that heading, as its own paragraph, rather
//! than under a second identical one. Only the top heading is checked, and it
//! is read from the file rather than remembered, because the file changes
//! underneath Talkie between captures.
//!
//! "The top" is not byte zero: YAML frontmatter and a leading `#` title stay
//! where they are. Inserting above frontmatter would break Obsidian integration.

use serde::{Deserialize, Serialize};

/// Render one entry: an H2 of the timestamp, then the text.
///
/// The caller supplies the timestamp already formatted — this crate compiles to
/// wasm as well as native, and a clock is not something it should own.
pub fn format_entry(text: &str, timestamp: &str) -> String {
    format!("## {timestamp}\n{}\n", text.trim())
}

/// Add a capture to `text`: under the top heading when that heading is
/// already `timestamp`'s minute, otherwise as a new entry at the insertion
/// point.
///
/// Either way the result is a pure insertion that [`inserted_at_head`]
/// recognises, which is what lets the editor's save carry a capture over.
pub fn capture(text: &str, body: &str, timestamp: &str) -> String {
    let heading = format!("## {timestamp}");
    match top_heading(text) {
        Some((top, at)) if top == heading => {
            let (head, tail) = text.split_at(at);
            let out = format!("{head}{}{tail}", paragraph(body, tail));
            debug_assert!(
                inserted_at_head(text, &out).is_some(),
                "joining a heading was not an insertion at the head"
            );
            out
        }
        _ => splice(text, &format_entry(body, timestamp)),
    }
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
    debug_assert!(
        text.is_char_boundary(offset),
        "insertion offset {offset} splits a character"
    );
    offset
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
    let out = normalized(&out);
    debug_assert!(
        out.len() >= text.len() + entry.trim_end_matches('\n').len(),
        "splice lost text"
    );
    out
}

/// Text that arrived at the head of the document: a new entry at the insertion
/// point, or a paragraph under the top heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inserted<'a> {
    /// The byte offset in the older text the insertion was made at.
    pub at: usize,
    /// What was inserted there.
    pub text: &'a str,
    /// The top heading `text` went under, or `None` when it went in at the
    /// insertion point.
    pub under: Option<&'a str>,
}

/// What was inserted at the head of `base` to produce `newer` — if an insertion
/// at the head is all that happened.
///
/// This is how a capture is recognised, and the head has two places in it: the
/// insertion point, where a new entry goes, and just under the top heading,
/// where a capture from the same minute goes. It is deliberately strict:
/// everything either side of one of those places must be untouched, or the
/// change was something else and the caller must not treat it as a capture.
pub fn inserted_at_head<'a>(base: &'a str, newer: &'a str) -> Option<Inserted<'a>> {
    if newer.len() <= base.len() {
        return None;
    }
    let at = insertion_offset(base);
    if let Some(text) = inserted_at(base, newer, at) {
        return Some(Inserted {
            at,
            text,
            under: None,
        });
    }
    let (heading, at) = top_heading(base)?;
    inserted_at(base, newer, at).map(|text| Inserted {
        at,
        text,
        under: Some(heading),
    })
}

/// Where `inserted` belongs in `target`, a different version of the document
/// from the one it was inserted into, and the text to put there.
///
/// A paragraph that went under a heading goes under the same heading in
/// `target` if `target` still opens with it, and otherwise brings the heading
/// with it as a new entry — the heading may have been edited away, but the
/// capture must not be.
pub fn carry_over(target: &str, inserted: &Inserted) -> (usize, String) {
    let Some(heading) = inserted.under else {
        return (insertion_offset(target), inserted.text.to_string());
    };
    match top_heading(target) {
        Some((top, at)) if top == heading => (at, paragraph(inserted.text, &target[at..])),
        _ => {
            let at = insertion_offset(target);
            let (head, tail) = target.split_at(at);
            let lead = if head.is_empty() || head.ends_with("\n\n") {
                ""
            } else if head.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            let entry = format!("{heading}\n{}", paragraph(inserted.text, tail));
            (at, format!("{lead}{entry}"))
        }
    }
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
        // Something arrived at the top. Put it at the top of the editor's text
        // too, wherever that text's own insertion point now is.
        Some(base) => match inserted_at_head(base, on_disk) {
            Some(Inserted {
                text, under: None, ..
            }) => Save::Write(splice(&incoming, text)),
            Some(inserted) => {
                let (at, text) = carry_over(&incoming, &inserted);
                let (head, tail) = incoming.split_at(at);
                Save::Write(normalized(&format!("{head}{text}{tail}")))
            }
            None => Save::Conflict,
        },
        // The editor never read the file, so there is no basis for a merge and
        // nothing to preserve.
        None => Save::Write(incoming),
    }
}

/// What lies between `base[..at]` and `base[at..]` in `newer`, if `newer` is
/// `base` with something inserted at `at`.
fn inserted_at<'a>(base: &str, newer: &'a str, at: usize) -> Option<&'a str> {
    let (head, tail) = base.split_at(at);
    if !newer.starts_with(head) || !newer.ends_with(tail) {
        return None;
    }
    let rest = &newer[head.len()..];
    let inserted_len = rest.len().checked_sub(tail.len())?;
    let inserted = &rest[..inserted_len];
    debug_assert_eq!(
        newer.len(),
        base.len() + inserted.len(),
        "the insertion does not account for the whole difference"
    );
    Some(inserted)
}

/// The heading of the top entry, trimmed, and the offset just under it — past
/// the blank lines that follow, so a paragraph put there does not stack another
/// on top of them.
///
/// `None` when the document does not open with an entry, or when the heading
/// is the file's last line and has no newline to put anything after.
fn top_heading(text: &str) -> Option<(&str, usize)> {
    let start = insertion_offset(text);
    let line = line_at(text, start);
    if !line.starts_with("## ") || !line.ends_with('\n') {
        return None;
    }
    let mut at = start + line.len();
    while at < text.len() {
        let line = line_at(text, at);
        if !line.trim().is_empty() {
            break;
        }
        at += line.len();
    }
    Some((line.trim_end(), at))
}

/// `body` as a paragraph to put in front of `tail`: trimmed, and followed by
/// the blank line that separates it from whatever comes next.
fn paragraph(body: &str, tail: &str) -> String {
    let body = body.trim();
    if tail.is_empty() {
        format!("{body}\n")
    } else {
        format!("{body}\n\n")
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
    debug_assert!(
        text.is_char_boundary(from),
        "line offset {from} splits a character"
    );
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
            Some(Inserted {
                at: 0,
                text: "## 2026-08-18 09:41\nNewest\n\n",
                under: None,
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

    // Issue #5: a save merged against a base that belongs to another file. The
    // three below pin the premises any fix has to work with.

    /// The editor still holds file A when the path moves to B. Merging its
    /// text against A's base refuses — B is not A with a capture on top —
    /// and that refusal is the only thing standing between the stale text
    /// and B.
    #[test]
    fn a_save_against_another_files_base_is_a_conflict() {
        let a = "## 2026-10-01 09:00\nOld file\n";
        let b = "## 2026-10-02 10:00\nNew file\n";
        let edited = format!("{a}typed\n");
        assert_eq!(reconcile(&edited, b, Some(a)), Save::Conflict);
        // A B that does not exist yet reads as empty — still a conflict.
        assert_eq!(reconcile(&edited, "", Some(a)), Save::Conflict);
    }

    /// Once the base has caught up with B, the stale text wins outright: a
    /// base equal to what is on disk means "nothing moved", whoever moved
    /// the base.
    #[test]
    fn a_base_that_caught_up_with_disk_writes_blindly() {
        let b = "## 2026-10-02 10:00\nNew file\n";
        let stale = "## 2026-10-01 09:00\nOld file\ntyped\n";
        assert_eq!(reconcile(stale, b, Some(b)), Save::Write(stale.to_string()));
    }

    /// So forgetting the base on a path change is not a fix on its own: with
    /// no base there is nothing to merge against, and the stale text is
    /// written straight over B.
    #[test]
    fn a_forgotten_base_writes_blindly_over_another_file() {
        let b = "## 2026-10-02 10:00\nNew file\n";
        let stale = "## 2026-10-01 09:00\nOld file\ntyped\n";
        assert_eq!(reconcile(stale, b, None), Save::Write(stale.to_string()));
    }

    #[test]
    fn a_capture_in_a_new_minute_starts_a_new_entry() {
        let existing = "## 2026-08-18 09:14\nOlder\n";
        assert_eq!(
            capture(existing, "Newest", "2026-08-18 09:41"),
            "## 2026-08-18 09:41\nNewest\n\n## 2026-08-18 09:14\nOlder\n"
        );
    }

    /// The point of the change: a second capture in the same minute shares the
    /// heading, newest paragraph first.
    #[test]
    fn a_capture_in_the_same_minute_joins_the_top_heading() {
        let existing = "## 2026-08-18 09:14\nFirst\n\n## 2026-08-18 09:02\nOlder\n";
        assert_eq!(
            capture(existing, "  Second  ", "2026-08-18 09:14"),
            "## 2026-08-18 09:14\nSecond\n\nFirst\n\n## 2026-08-18 09:02\nOlder\n"
        );
    }

    #[test]
    fn joining_respects_frontmatter_and_a_title() {
        let existing = "---\ntags: [talkie]\n---\n\n# talkie.md\n\n## 2026-08-18 09:14\nFirst\n";
        assert_eq!(
            capture(existing, "Second", "2026-08-18 09:14"),
            "---\ntags: [talkie]\n---\n\n# talkie.md\n\n## 2026-08-18 09:14\nSecond\n\nFirst\n"
        );
    }

    /// Only the top heading counts. The same minute further down — an older
    /// file, a hand edit — is not reached into.
    #[test]
    fn only_the_top_heading_is_joined() {
        let existing = "## 2026-08-18 09:20\nTop\n\n## 2026-08-18 09:14\nFirst\n";
        assert!(capture(existing, "Second", "2026-08-18 09:14")
            .starts_with("## 2026-08-18 09:14\nSecond\n\n## 2026-08-18 09:20\n"));
    }

    #[test]
    fn joining_an_empty_heading_leaves_no_trailing_blank_line() {
        assert_eq!(
            capture("## 2026-08-18 09:14\n", "Second", "2026-08-18 09:14"),
            "## 2026-08-18 09:14\nSecond\n"
        );
    }

    #[test]
    fn joining_never_stacks_blank_lines() {
        let existing = "## 2026-08-18 09:14\n\n\nFirst\n";
        let joined = capture(existing, "Second", "2026-08-18 09:14");
        assert_eq!(joined, "## 2026-08-18 09:14\n\n\nSecond\n\nFirst\n");
        assert!(inserted_at_head(existing, &joined).is_some());
    }

    #[test]
    fn recognises_a_capture_that_joined_the_top_heading() {
        let base = "# talkie.md\n\n## 2026-08-18 09:14\nFirst\n";
        let newer = capture(base, "Second", "2026-08-18 09:14");
        assert_eq!(
            inserted_at_head(base, &newer),
            Some(Inserted {
                at: "# talkie.md\n\n## 2026-08-18 09:14\n".len(),
                text: "Second\n\n",
                under: Some("## 2026-08-18 09:14"),
            })
        );
    }

    /// Typing in the top entry while a same-minute capture lands: both stay,
    /// and the capture is under the heading it was spoken into.
    #[test]
    fn a_joined_capture_that_lands_mid_edit_is_carried_over() {
        let base = "## 2026-08-18 09:14\nFirst\n";
        let on_disk = capture(base, "Second", "2026-08-18 09:14");
        let incoming = "## 2026-08-18 09:14\nFirst, and edited\n";

        assert_eq!(
            reconcile(incoming, &on_disk, Some(base)),
            Save::Write("## 2026-08-18 09:14\nSecond\n\nFirst, and edited\n".to_string())
        );
    }

    /// The editor deleted the entry the capture joined. The capture survives,
    /// and brings its heading back with it.
    #[test]
    fn a_joined_capture_outlives_its_heading_being_deleted() {
        let base = "## 2026-08-18 09:14\nFirst\n\n## 2026-08-18 09:02\nOlder\n";
        let on_disk = capture(base, "Second", "2026-08-18 09:14");
        let incoming = "## 2026-08-18 09:02\nOlder\n";

        assert_eq!(
            reconcile(incoming, &on_disk, Some(base)),
            Save::Write("## 2026-08-18 09:14\nSecond\n\n## 2026-08-18 09:02\nOlder\n".to_string())
        );
    }

    #[test]
    fn a_resurrected_heading_keeps_its_distance_from_a_title() {
        let base = "## 2026-08-18 09:14\nFirst\n";
        let on_disk = capture(base, "Second", "2026-08-18 09:14");
        let incoming = "# talkie.md\n";

        assert_eq!(
            reconcile(incoming, &on_disk, Some(base)),
            Save::Write("# talkie.md\n\n## 2026-08-18 09:14\nSecond\n".to_string())
        );
    }

    /// Carrying over into the very text it was inserted into reproduces the
    /// insertion exactly — the editor relies on that to stay byte-identical to
    /// the file.
    #[test]
    fn carrying_over_into_the_base_is_exact() {
        let bases = [
            "## 2026-08-18 09:14\nFirst\n",
            "## 2026-08-18 09:14\n",
            "# talkie.md\n\n## 2026-08-18 09:14\n\nFirst\n\n## 2026-08-18 09:02\nOlder\n",
        ];
        for base in bases {
            let newer = capture(base, "Second", "2026-08-18 09:14");
            let inserted = inserted_at_head(base, &newer).expect("not recognised");
            let (at, text) = carry_over(base, &inserted);
            assert_eq!(at, inserted.at, "base: {base:?}");
            assert_eq!(text, inserted.text, "base: {base:?}");
        }
    }

    #[test]
    fn entry_shape_matches_the_contract() {
        assert_eq!(
            format_entry("  Some words  ", "2026-08-18 09:14"),
            "## 2026-08-18 09:14\nSome words\n"
        );
    }
}
