//! Helpers shared by the host's tests.

use std::io::ErrorKind;
use std::path::Path;

/// Remove a scratch directory left by an earlier run, if there is one.
///
/// Absent is the usual case and fine. Anything else fails the test: a scratch
/// directory that cannot be cleared would let the test read the last run's
/// files.
pub fn remove_dir_if_present(dir: &Path) {
    if let Err(e) = std::fs::remove_dir_all(dir) {
        assert_eq!(
            e.kind(),
            ErrorKind::NotFound,
            "could not clear {}: {e}",
            dir.display()
        );
    }
}
