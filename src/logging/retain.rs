use std::fs;
use std::path::{Path, PathBuf};

/// Scan a directory for log files matching the given prefix and delete files
/// according to the retention policy.
///
/// Returns a list of files that could not be deleted (path + error).
/// The caller is responsible for surfacing these errors.
pub(crate) fn cleanup_old_logs(
    dir: &Path,
    prefix: &str,
    retain_days: Option<u32>,
    retain_files: Option<u32>,
) -> Vec<(PathBuf, std::io::Error)> {
    debug_assert!(
        !(retain_days.is_some() && retain_files.is_some()),
        "retain_days and retain_files are mutually exclusive"
    );

    let mut errors = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            // Nonexistent directory is a no-op (no files to clean).
            // Other errors (e.g. permission denied) are surfaced to the caller.
            if e.kind() != std::io::ErrorKind::NotFound {
                errors.push((dir.to_path_buf(), e));
            }
            return errors;
        }
    };

    // Collect matching files with their modification times
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let name = path.file_name()?.to_str()?;
            // Match "{prefix}." to avoid matching unrelated files (e.g. "app" matching "application.log").
            // Rotated files always have the format "{prefix}.{date}", so the dot is always present.
            let match_prefix = format!("{prefix}.");
            if !name.starts_with(&match_prefix) {
                return None;
            }
            let mtime = entry.metadata().ok()?.modified().ok()?;
            Some((path, mtime))
        })
        .collect();

    // Sort by modification time, newest first
    files.sort_by(|a, b| b.1.cmp(&a.1));

    if let Some(days) = retain_days {
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(u64::from(days) * 24 * 3600);
        for (path, mtime) in &files {
            if *mtime < cutoff {
                if let Err(e) = fs::remove_file(path) {
                    errors.push((path.clone(), e));
                }
            }
        }
    }

    if let Some(max_files) = retain_files {
        let max = max_files as usize;
        if files.len() > max {
            for (path, _) in &files[max..] {
                if let Err(e) = fs::remove_file(path) {
                    errors.push((path.clone(), e));
                }
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_old_logs_days_retention() {
        let dir = tempfile::tempdir().unwrap();

        // Create files with different modification times
        let old_file = dir.path().join("app.2024-01-01.log");
        let new_file = dir.path().join("app.2024-12-01.log");
        fs::write(&old_file, "old").unwrap();
        fs::write(&new_file, "new").unwrap();

        // Set the old file's mtime to 30 days ago
        let old_time = std::time::SystemTime::now()
            - std::time::Duration::from_secs(30 * 24 * 3600);
        let times = fs::FileTimes::new().set_modified(old_time);
        fs::File::open(&old_file).unwrap().set_times(times).unwrap();

        let errors = cleanup_old_logs(dir.path(), "app", Some(7), None);
        assert!(errors.is_empty());
        assert!(!old_file.exists(), "old file should be deleted");
        assert!(new_file.exists(), "new file should be kept");
    }

    #[test]
    fn cleanup_old_logs_files_retention() {
        let dir = tempfile::tempdir().unwrap();

        // Create 5 files with staggered mtimes
        let mut files = Vec::new();
        for i in 0..5 {
            let path = dir.path().join(format!("app.{i}.log"));
            fs::write(&path, format!("log {i}")).unwrap();
            let mtime = std::time::SystemTime::now()
                - std::time::Duration::from_secs((5 - i) * 3600);
            let times = fs::FileTimes::new().set_modified(mtime);
            fs::File::open(&path).unwrap().set_times(times).unwrap();
            files.push(path);
        }

        // Keep only 3 most recent
        let errors = cleanup_old_logs(dir.path(), "app", None, Some(3));
        assert!(errors.is_empty());

        // Files 3, 4 (newest) should exist; 0, 1 (oldest) should be deleted
        assert!(!files[0].exists(), "oldest file should be deleted");
        assert!(!files[1].exists(), "second oldest should be deleted");
        assert!(files[2].exists(), "third newest should be kept");
        assert!(files[3].exists(), "second newest should be kept");
        assert!(files[4].exists(), "newest should be kept");
    }

    #[test]
    fn cleanup_old_logs_nonexistent_dir() {
        let errors = cleanup_old_logs(Path::new("/nonexistent/dir"), "app", Some(7), None);
        assert!(errors.is_empty());
    }

    #[test]
    fn cleanup_old_logs_non_matching_prefix_ignored() {
        let dir = tempfile::tempdir().unwrap();

        let matching = dir.path().join("app.old.log");
        let non_matching = dir.path().join("other.old.log");
        fs::write(&matching, "match").unwrap();
        fs::write(&non_matching, "no match").unwrap();

        // Set both to old times
        let old_time = std::time::SystemTime::now()
            - std::time::Duration::from_secs(30 * 24 * 3600);
        let times = fs::FileTimes::new().set_modified(old_time);
        fs::File::open(&matching)
            .unwrap()
            .set_times(times)
            .unwrap();
        fs::File::open(&non_matching)
            .unwrap()
            .set_times(times)
            .unwrap();

        let errors = cleanup_old_logs(dir.path(), "app", Some(7), None);
        assert!(errors.is_empty());
        assert!(!matching.exists(), "matching old file should be deleted");
        assert!(
            non_matching.exists(),
            "non-matching file should be untouched"
        );
    }
}
