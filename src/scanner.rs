use std::{
    error::Error,
    fs,
    io,
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// Recursively scans the given start path for files.
/// Returns a vector of FileEntry sorted in descending order by file size.
/// Errors are wrapped with context and marked as Send + 'static.
pub fn scan_files(start_path: &str) -> Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>> {
    let mut files = Vec::new();

    fn visit_dirs(dir: &Path, files: &mut Vec<FileEntry>) -> Result<(), Box<dyn Error + Send + 'static>> {
        let entries = fs::read_dir(dir).map_err(|e| -> Box<dyn Error + Send + 'static> {
            Box::new(io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to read directory {:?}: {}", dir, e),
            ))
        })?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("Failed to get directory entry in {:?}: {}", dir, e);
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                if let Err(e) = visit_dirs(&path, files) {
                    if let Some(io_err) = e.downcast_ref::<io::Error>() {
                        if io_err.kind() == io::ErrorKind::PermissionDenied {
                            eprintln!("Permission denied for directory: {:?}", path);
                            continue;
                        }
                    }
                    return Err(e);
                }
            } else if path.is_file() {
                let metadata = match fs::metadata(&path) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Failed to get metadata for {:?}: {}", path, e);
                        continue;
                    }
                };
                let size = metadata.len();
                files.push(FileEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: path.to_string_lossy().into_owned(),
                    size,
                });
            }
        }
        Ok(())
    }

    visit_dirs(Path::new(start_path), &mut files)
        .map_err(|e| -> Box<dyn Error + Send + 'static> { e })?;
    files.sort_by(|a, b| b.size.cmp(&a.size));
    Ok(files)
}
