use std::{error::Error, fs, path::Path, io};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// Scans for files under the given `start_path` using the WalkDir crate.
/// This implementation iterates recursively over directories, skips over errors gracefully,
/// obtains file metadata, and returns a vector of FileEntry items sorted in descending order by file size.
/// Errors are wrapped to satisfy `Send + 'static` and are returned only if the traversal itself fails catastrophically.
pub fn scan_files(start_path: &str) -> Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(start_path)
        .into_iter()
        // Filter out errors (or you could log them instead)
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            match fs::metadata(path) {
                Ok(metadata) => {
                    let size = metadata.len();
                    let name = path
                        .file_name()
                        .map(|os_str| os_str.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned());
                    files.push(FileEntry {
                        name,
                        path: path.to_string_lossy().into_owned(),
                        size,
                    });
                },
                Err(e) => {
                    eprintln!("Failed to read metadata for {:?}: {}", path, e);
                    // Optionally continue scanning even if a file’s metadata cannot be read.
                    continue;
                }
            }
        }
    }

    files.sort_by(|a, b| b.size.cmp(&a.size));
    Ok(files)
}

/// Lists the contents of the directory at `start_path` (non-recursively) using WalkDir.
pub fn list_directory(start_path: &str) -> Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>> {
    let mut entries = Vec::new();
    // Use WalkDir with max_depth = 1 to list only immediate children.
    for entry in WalkDir::new(start_path).max_depth(1).into_iter().filter_map(|e| e.ok()) {
        // Skip the directory itself.
        if entry.path() == Path::new(start_path) {
            continue;
        }
        // Process files and directories.
        if entry.path().is_file() || entry.path().is_dir() {
            let metadata = fs::metadata(entry.path()).map_err(|e| {
                Box::new(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to get metadata for {:?}: {}", entry.path(), e),
                )) as Box<dyn Error + Send + 'static>
            })?;
            let size = metadata.len();
            let name = entry
                .path()
                .file_name()
                .map(|os_str| os_str.to_string_lossy().into_owned())
                .unwrap_or_default();
            entries.push(FileEntry {
                name,
                path: entry.path().to_string_lossy().into_owned(),
                size,
            });
        }
    }
    // Optionally sort entries by name or by size.
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}