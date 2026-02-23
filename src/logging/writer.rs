use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use flate2::Compression;
use time::macros::format_description;
use time::OffsetDateTime;

use super::error::LoggingError;
use super::retain;

/// A size-based rotating log writer.
///
/// Writes to an active log file at `{dir}/{prefix}`. When the file exceeds
/// `max_bytes`, the active file is renamed with a UTC timestamp suffix and a
/// new active file is opened. Optionally compresses rotated files and runs
/// retention cleanup after each rotation.
///
/// Designed to be used inside `tracing_appender::non_blocking`, which serializes
/// all writes through a single worker thread. No internal locking is needed.
pub(crate) struct SizeRotatingWriter {
    file: File,
    bytes_written: u64,
    max_bytes: u64,
    dir: PathBuf,
    prefix: String,
    compress: bool,
    retain_days: Option<u32>,
    retain_files: Option<u32>,
}

impl SizeRotatingWriter {
    /// Create a new size-rotating writer.
    ///
    /// Opens or creates `{dir}/{prefix}` in append mode. If the file already
    /// exists, the byte counter is recovered from the file's current size.
    pub(crate) fn new(
        dir: PathBuf,
        prefix: String,
        max_bytes: u64,
        compress: bool,
        retain_days: Option<u32>,
        retain_files: Option<u32>,
    ) -> Result<Self, LoggingError> {
        let active_path = dir.join(&prefix);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active_path)
            .map_err(|e| LoggingError::FileSetupFailed {
                dir: dir.clone(),
                source: e,
            })?;

        let bytes_written = file
            .metadata()
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(Self {
            file,
            bytes_written,
            max_bytes,
            dir,
            prefix,
            compress,
            retain_days,
            retain_files,
        })
    }

    /// The path of the active log file.
    fn active_path(&self) -> PathBuf {
        self.dir.join(&self.prefix)
    }

    /// Rotate the active log file: rename it with a timestamp, open a new one,
    /// optionally compress, and run retention.
    ///
    /// All errors are soft — logged via `eprintln!` and the writer continues.
    /// On failure, `bytes_written` is reset to 0 to prevent re-triggering
    /// `rotate()` on every subsequent write call.
    fn rotate(&mut self) {
        // Reset counter immediately to prevent cascading re-rotation on failure.
        // If rotation succeeds, this is overwritten to 0 anyway. If it fails,
        // writes continue to the current file (or the renamed-away fd on Linux)
        // without re-triggering rotation on every call.
        self.bytes_written = 0;

        if let Err(e) = self.file.flush() {
            eprintln!(
                "dragon-fnd: failed to flush log file before rotation: {e}"
            );
        }

        let rotated_path = match self.generate_rotated_path() {
            Ok(path) => path,
            Err(e) => {
                eprintln!(
                    "dragon-fnd: failed to generate rotated filename: {e}"
                );
                return;
            }
        };

        if let Err(e) = fs::rename(self.active_path(), &rotated_path) {
            eprintln!(
                "dragon-fnd: failed to rename log file to '{}': {e}",
                rotated_path.display()
            );
            return;
        }

        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.active_path())
        {
            Ok(file) => {
                self.file = file;
            }
            Err(e) => {
                eprintln!(
                    "dragon-fnd: failed to open new log file '{}': {e}",
                    self.active_path().display()
                );
                return;
            }
        }

        if self.compress {
            let path = rotated_path.clone();
            std::thread::spawn(move || {
                if let Err(e) = compress_file(&path) {
                    eprintln!(
                        "dragon-fnd: failed to compress {}: {e}",
                        path.display()
                    );
                }
            });
        }

        let errors = retain::cleanup_old_logs(
            &self.dir,
            &self.prefix,
            self.retain_days,
            self.retain_files,
        );
        for (path, err) in errors {
            eprintln!(
                "dragon-fnd: failed to remove old log file '{}': {err}",
                path.display()
            );
        }
    }

    /// Generate a unique timestamped path for the rotated file.
    fn generate_rotated_path(&self) -> io::Result<PathBuf> {
        let now = OffsetDateTime::now_utc();
        let format = format_description!(
            "[year][month][day]T[hour][minute][second].[subsecond digits:3]"
        );
        let timestamp = now
            .format(&format)
            .map_err(io::Error::other)?;

        let base = format!("{}.{timestamp}", self.prefix);
        let base_path = self.dir.join(&base);

        if !base_path.exists() {
            return Ok(base_path);
        }

        // Collision fallback: append .1, .2, etc.
        for suffix in 1u32.. {
            let candidate = self.dir.join(format!("{base}.{suffix}"));
            if !candidate.exists() {
                return Ok(candidate);
            }
        }

        unreachable!("exhausted u32 collision suffixes")
    }
}

impl Write for SizeRotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.file.write(buf)?;
        self.bytes_written += n as u64;
        if self.bytes_written >= self.max_bytes {
            self.rotate();
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Compress a file with gzip, writing to `{path}.gz`, then delete the original.
///
/// Uses streaming compression to avoid loading the entire file into memory.
fn compress_file(path: &Path) -> io::Result<()> {
    let mut gz_name = path.as_os_str().to_os_string();
    gz_name.push(".gz");
    let gz_path = PathBuf::from(gz_name);

    let input = File::open(path)?;
    let mut reader = io::BufReader::new(input);
    let out_file = File::create(&gz_path)?;
    let mut encoder = GzEncoder::new(out_file, Compression::default());
    io::copy(&mut reader, &mut encoder)?;
    encoder.finish()?;

    // Delete original; silently ignore NotFound (retention may have already removed it)
    if let Err(e) = fs::remove_file(path) {
        if e.kind() != io::ErrorKind::NotFound {
            return Err(e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_active_file() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SizeRotatingWriter::new(
            dir.path().to_path_buf(),
            "app".to_string(),
            4096,
            false,
            None,
            None,
        )
        .unwrap();

        assert!(dir.path().join("app").exists());
        assert_eq!(writer.bytes_written, 0);
    }

    #[test]
    fn new_recovers_byte_count_from_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("app"), "hello world").unwrap();

        let writer = SizeRotatingWriter::new(
            dir.path().to_path_buf(),
            "app".to_string(),
            4096,
            false,
            None,
            None,
        )
        .unwrap();

        assert_eq!(writer.bytes_written, 11);
    }

    #[test]
    fn write_increments_counter() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = SizeRotatingWriter::new(
            dir.path().to_path_buf(),
            "app".to_string(),
            4096,
            false,
            None,
            None,
        )
        .unwrap();

        let n = writer.write(b"hello").unwrap();
        assert_eq!(n, 5);
        assert_eq!(writer.bytes_written, 5);

        let n = writer.write(b" world").unwrap();
        assert_eq!(n, 6);
        assert_eq!(writer.bytes_written, 11);
    }

    #[test]
    fn rotation_triggered_at_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = SizeRotatingWriter::new(
            dir.path().to_path_buf(),
            "app".to_string(),
            10, // 10-byte threshold
            false,
            None,
            None,
        )
        .unwrap();

        // Write enough to trigger rotation
        writer.write_all(b"0123456789X").unwrap();

        // Active file should be fresh (0 or small)
        assert_eq!(writer.bytes_written, 0);

        // There should be a rotated file matching "app."
        let rotated: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("app.") && name != "app"
            })
            .collect();
        assert_eq!(rotated.len(), 1);

        // Rotated file should contain the original data
        let rotated_content = fs::read_to_string(rotated[0].path()).unwrap();
        assert_eq!(rotated_content, "0123456789X");
    }

    #[test]
    fn multiple_rapid_rotations_produce_unique_filenames() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = SizeRotatingWriter::new(
            dir.path().to_path_buf(),
            "app".to_string(),
            5, // very small threshold
            false,
            None,
            None,
        )
        .unwrap();

        // Trigger multiple rotations rapidly
        for _ in 0..5 {
            writer.write_all(b"12345X").unwrap();
        }

        // Should have 5 rotated files + 1 active
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 6, "expected 5 rotated + 1 active");

        // All filenames should be unique
        let mut names: Vec<String> = entries
            .iter()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 6, "all filenames should be unique");
    }

    #[test]
    fn timestamp_format_is_correct_shape() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = SizeRotatingWriter::new(
            dir.path().to_path_buf(),
            "app".to_string(),
            5,
            false,
            None,
            None,
        )
        .unwrap();

        writer.write_all(b"12345X").unwrap();

        let rotated: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("app.") && name != "app"
            })
            .collect();
        assert_eq!(rotated.len(), 1);

        // Format: app.YYYYMMDDTHHmmss.SSS
        let name = rotated[0].file_name().to_string_lossy().to_string();
        let timestamp = name.strip_prefix("app.").unwrap();
        assert_eq!(timestamp.len(), 19, "timestamp should be YYYYMMDDTHHmmss.SSS (19 chars)");
        assert_eq!(&timestamp[8..9], "T", "should contain T separator");
        assert_eq!(&timestamp[15..16], ".", "should contain dot before millis");
    }

    #[test]
    fn rotation_with_compression_produces_gz() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = SizeRotatingWriter::new(
            dir.path().to_path_buf(),
            "app".to_string(),
            10,
            true, // compress enabled
            None,
            None,
        )
        .unwrap();

        writer.write_all(b"0123456789X").unwrap();

        // Wait for background compression thread
        std::thread::sleep(std::time::Duration::from_millis(500));

        let gz_files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with(".gz")
            })
            .collect();
        assert_eq!(gz_files.len(), 1, "should have one .gz file");

        // Original uncompressed rotated file should be deleted
        let uncompressed_rotated: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("app.") && !name.ends_with(".gz")
            })
            .collect();
        assert_eq!(
            uncompressed_rotated.len(),
            0,
            "original rotated file should be deleted after compression"
        );
    }

    #[test]
    fn compress_file_produces_valid_gzip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.log");
        let content = "hello world, this is a log line\n";
        fs::write(&path, content).unwrap();

        compress_file(&path).unwrap();

        assert!(!path.exists(), "original should be deleted");

        let gz_path = dir.path().join("test.log.gz");
        assert!(gz_path.exists(), "gz file should exist");

        // Decompress and verify
        use flate2::read::GzDecoder;
        use std::io::Read;
        let gz_data = fs::read(&gz_path).unwrap();
        let mut decoder = GzDecoder::new(&gz_data[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert_eq!(decompressed, content);
    }

    #[test]
    fn retention_runs_inline_after_rotation() {
        let dir = tempfile::tempdir().unwrap();

        // Pre-create some old rotated files
        for i in 0..5 {
            let path = dir.path().join(format!("app.old{i}"));
            fs::write(&path, "old log").unwrap();
            let old_time = std::time::SystemTime::now()
                - std::time::Duration::from_secs((10 - i) * 3600);
            let times = fs::FileTimes::new().set_modified(old_time);
            fs::File::open(&path).unwrap().set_times(times).unwrap();
        }

        let mut writer = SizeRotatingWriter::new(
            dir.path().to_path_buf(),
            "app".to_string(),
            10,
            false,
            None,
            Some(2), // keep only 2 most recent
        )
        .unwrap();

        // Trigger rotation — this also runs retention
        writer.write_all(b"0123456789X").unwrap();

        // Count remaining rotated files (excluding active "app")
        let rotated: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("app.") && name != "app"
            })
            .collect();

        assert!(
            rotated.len() <= 2,
            "retention should keep at most 2 files, got {}",
            rotated.len()
        );
    }

    #[test]
    fn compressed_gz_files_counted_by_retention() {
        let dir = tempfile::tempdir().unwrap();

        // Pre-create some .gz files (simulating previous compressed rotations)
        for i in 0..5 {
            let path = dir.path().join(format!("app.old{i}.gz"));
            fs::write(&path, "fake gz").unwrap();
            let old_time = std::time::SystemTime::now()
                - std::time::Duration::from_secs((10 - i) * 3600);
            let times = fs::FileTimes::new().set_modified(old_time);
            fs::File::open(&path).unwrap().set_times(times).unwrap();
        }

        // Run retention: keep only 2
        let errors = retain::cleanup_old_logs(
            dir.path(),
            "app",
            None,
            Some(2),
        );
        assert!(errors.is_empty());

        let remaining: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("app.")
            })
            .collect();
        assert_eq!(remaining.len(), 2, "should keep exactly 2 .gz files");
    }
}
