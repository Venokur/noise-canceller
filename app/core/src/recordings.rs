//! A/B pairs on disk.
//!
//! The `ab-<stamp>-raw.wav` / `ab-<stamp>-clean.wav` / `ab-<stamp>.json` naming
//! used to live in five places (the writer, the history list, the "last take"
//! lookup, deletion and pruning). One typo in one of them meant an orphan file
//! that only pruning noticed.
//!
//! The `*_in` variants take the folder explicitly so the rules above can be
//! tested without touching `data/recordings`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::paths;

pub fn dir() -> PathBuf {
    paths::recordings_dir()
}

/// A long clean-only take, separate from the A/B pairs.
pub fn clean_take(stamp: u64) -> PathBuf {
    dir().join(format!("clean-{stamp}.wav"))
}

fn raw_in(dir: &Path, stamp: u64) -> PathBuf {
    dir.join(format!("ab-{stamp}-raw.wav"))
}

fn clean_in(dir: &Path, stamp: u64) -> PathBuf {
    dir.join(format!("ab-{stamp}-clean.wav"))
}

fn snapshot_in(dir: &Path, stamp: u64) -> PathBuf {
    dir.join(format!("ab-{stamp}.json"))
}

/// Files belonging to one take: the pair and its settings snapshot.
fn files_in(dir: &Path, stamp: u64) -> [PathBuf; 3] {
    [
        raw_in(dir, stamp),
        clean_in(dir, stamp),
        snapshot_in(dir, stamp),
    ]
}

pub fn raw(stamp: u64) -> PathBuf {
    raw_in(&dir(), stamp)
}

pub fn clean(stamp: u64) -> PathBuf {
    clean_in(&dir(), stamp)
}

/// The `Settings` a take was recorded with, so it can be replayed later.
pub fn snapshot(stamp: u64) -> PathBuf {
    snapshot_in(&dir(), stamp)
}

/// Both files of one take. Resolves the folder once instead of once per path.
pub fn pair(stamp: u64) -> (PathBuf, PathBuf) {
    let dir = dir();
    (raw_in(&dir, stamp), clean_in(&dir, stamp))
}

fn stamp_of(name: &str, suffix: &str) -> Option<u64> {
    name.strip_prefix("ab-")?.strip_suffix(suffix)?.parse().ok()
}

fn pairs_in(dir: &Path) -> Vec<u64> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(stamp) = stamp_of(&name, "-raw.wav") else {
            continue;
        };
        // a raw file without its clean half is an interrupted take, not a pair
        if clean_in(dir, stamp).exists() {
            out.push(stamp);
        }
    }
    out.sort_unstable_by(|a, b| b.cmp(a));
    out
}

/// Stamps of complete pairs, newest first.
pub fn pairs() -> Vec<u64> {
    pairs_in(&dir())
}

fn newest_pair_in(dir: &Path) -> Option<(u64, PathBuf, PathBuf)> {
    let mut newest: Option<(std::time::SystemTime, u64)> = None;
    let read = std::fs::read_dir(dir).ok()?;

    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(stamp) = stamp_of(&name, "-raw.wav") else {
            continue;
        };
        let Some(time) = entry.metadata().ok().and_then(|m| m.modified().ok()) else {
            continue;
        };
        if newest.as_ref().map(|(t, _)| time > *t).unwrap_or(true) {
            newest = Some((time, stamp));
        }
    }

    let (_, stamp) = newest?;
    let clean = clean_in(dir, stamp);
    if !clean.exists() {
        return None;
    }
    Some((stamp, raw_in(dir, stamp), clean))
}

/// Newest complete pair, for the play buttons after a restart: the paths live
/// in the engine, which is new on every launch.
pub fn newest_pair() -> Option<(u64, PathBuf, PathBuf)> {
    newest_pair_in(&dir())
}

fn delete_in(dir: &Path, stamp: u64) -> Result<()> {
    for path in files_in(dir, stamp) {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

pub fn delete(stamp: u64) -> Result<()> {
    delete_in(&dir(), stamp)
}

fn prune_in(dir: &Path, keep: usize) -> usize {
    let mut stamps: Vec<u64> = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(stamp) = stamp_of(&name, "-clean.wav") {
                stamps.push(stamp);
            }
        }
    }
    stamps.sort_unstable_by(|a, b| b.cmp(a));

    let mut removed = 0;
    for stamp in stamps.into_iter().skip(keep.max(1)) {
        for path in files_in(dir, stamp) {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }

    // a snapshot whose pair was deleted by hand (or never written) is litter:
    // it has no stamp in `stamps`, so remove it here
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(text) = name
                .strip_prefix("ab-")
                .and_then(|rest| rest.strip_suffix(".json"))
            else {
                continue;
            };
            let stamp = text.parse::<u64>().unwrap_or(0);
            if !raw_in(dir, stamp).exists()
                && !clean_in(dir, stamp).exists()
                && std::fs::remove_file(entry.path()).is_ok()
            {
                removed += 1;
            }
        }
    }

    // a take interrupted by a quit leaves the half-written temp file behind
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("ab-")
                && name.ends_with(".wav.tmp")
                && std::fs::remove_file(entry.path()).is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
}

/// Keeps the newest `keep` takes and drops the rest, plus snapshots whose pair
/// was deleted by hand. Returns how many files were removed.
pub fn prune(keep: usize) -> usize {
    prune_in(&dir(), keep)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder outside `data/recordings`, so the rules can be tested
    /// without deleting a real take.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("nc-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Self(dir)
        }

        fn pair(&self, stamp: u64, snapshot: bool) {
            for path in [raw_in(&self.0, stamp), clean_in(&self.0, stamp)] {
                std::fs::write(path, b"wav").expect("write wav");
            }
            if snapshot {
                std::fs::write(snapshot_in(&self.0, stamp), b"{}").expect("write snapshot");
            }
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_complete_pairs_are_listed_newest_first() {
        let scratch = Scratch::new("pairs");
        scratch.pair(10, true);
        scratch.pair(20, true);
        // an interrupted take: the raw half alone must not become a pair
        std::fs::write(raw_in(scratch.path(), 30), b"wav").expect("write raw");

        assert_eq!(pairs_in(scratch.path()), vec![20, 10]);
    }

    #[test]
    fn newest_pair_skips_an_interrupted_take() {
        let scratch = Scratch::new("newest");
        assert!(newest_pair_in(scratch.path()).is_none());

        std::fs::write(raw_in(scratch.path(), 7), b"wav").expect("write raw");
        assert!(
            newest_pair_in(scratch.path()).is_none(),
            "a raw file without a clean one is not playable"
        );

        scratch.pair(7, true);
        let (stamp, raw, clean) = newest_pair_in(scratch.path()).expect("pair");
        assert_eq!(stamp, 7);
        assert_eq!(raw, raw_in(scratch.path(), 7));
        assert_eq!(clean, clean_in(scratch.path(), 7));
    }

    #[test]
    fn prune_keeps_the_newest_and_cleans_orphan_snapshots() {
        let scratch = Scratch::new("prune");
        for stamp in 1..=4 {
            scratch.pair(stamp, true);
        }
        // pair deleted by hand: the snapshot has no wav left
        std::fs::write(snapshot_in(scratch.path(), 9), b"{}").expect("write snapshot");
        // a take interrupted by a quit: only the half-written temp file is left
        std::fs::write(scratch.path().join("ab-8-raw.wav.tmp"), b"half").expect("write tmp");

        // three old takes (pair + snapshot each), the orphan snapshot and the
        // interrupted temp file
        assert_eq!(prune_in(scratch.path(), 2), 8);
        assert_eq!(pairs_in(scratch.path()), vec![4, 3]);
        assert!(!snapshot_in(scratch.path(), 9).exists());
        assert!(!scratch.path().join("ab-8-raw.wav.tmp").exists());
    }

    #[test]
    fn delete_removes_the_pair_and_its_snapshot() {
        let scratch = Scratch::new("delete");
        scratch.pair(5, true);
        scratch.pair(6, true);

        delete_in(scratch.path(), 5).expect("delete");
        assert_eq!(pairs_in(scratch.path()), vec![6]);
        assert!(!snapshot_in(scratch.path(), 5).exists());
    }
}
