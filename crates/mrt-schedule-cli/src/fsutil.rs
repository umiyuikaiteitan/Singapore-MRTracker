//! Atomic file output.
//!
//! A generator that is interrupted halfway must not leave a truncated
//! timetable where a reader expects a whole one. Every artifact is
//! written to a temporary file in the same directory and then renamed
//! over the target, which is atomic on every platform the project
//! supports.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{CliError, ExitCode};

/// Write bytes to `path` atomically.
///
/// The function creates the parent directory when it is missing.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let fail = |message: String| CliError::new(ExitCode::OutputFailure, message);

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| fail(format!("cannot create {}: {e}", parent.display())))?;
    }
    let temporary = temporary_path(path);
    {
        let mut file = std::fs::File::create(&temporary)
            .map_err(|e| fail(format!("cannot create {}: {e}", temporary.display())))?;
        file.write_all(bytes)
            .map_err(|e| fail(format!("cannot write {}: {e}", temporary.display())))?;
        // Flush to the operating system before the rename, so the
        // renamed file is never shorter than the bytes we wrote.
        file.flush()
            .map_err(|e| fail(format!("cannot flush {}: {e}", temporary.display())))?;
    }
    std::fs::rename(&temporary, path).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        fail(format!(
            "cannot move the output into {}: {e}",
            path.display()
        ))
    })
}

/// Write a UTF-8 string atomically.
pub fn write_atomic_str(path: &Path, text: &str) -> Result<(), CliError> {
    write_atomic(path, text.as_bytes())
}

/// Build the temporary path beside the target.
///
/// The name carries the process identifier, so two generators writing
/// to one directory do not collide.
fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string());
    let temporary = format!(".{name}.{}.tmp", std::process::id());
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(temporary),
        _ => PathBuf::from(temporary),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_creates_the_file_and_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep/nested/out.html");
        write_atomic_str(&path, "<html>").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "<html>");
    }

    #[test]
    fn writing_replaces_an_existing_file_completely() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        write_atomic_str(&path, "a longer first version").unwrap();
        write_atomic_str(&path, "short").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "short");
    }

    #[test]
    fn no_temporary_file_survives_a_successful_write() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic_str(&dir.path().join("out.txt"), "x").unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["out.txt".to_string()]);
    }

    #[test]
    fn the_temporary_name_sits_beside_the_target() {
        let temporary = temporary_path(Path::new("/tmp/dist/board.html"));
        assert_eq!(temporary.parent().unwrap(), Path::new("/tmp/dist"));
        assert!(temporary
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".board.html."));
    }

    #[test]
    fn a_directory_target_fails_with_the_output_code() {
        let dir = tempfile::tempdir().unwrap();
        let error = write_atomic(dir.path(), b"x").unwrap_err();
        assert_eq!(error.exit, ExitCode::OutputFailure);
    }
}
