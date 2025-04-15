use std::{error::Error, path::Path, io, sync::Arc, time::Duration};
use jwalk::{WalkDir, Parallelism};
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// Scans for files under the given `start_path` using jwalk for parallel directory traversal.
/// This implementation iterates recursively over directories in parallel, skips over errors gracefully,
/// obtains file metadata, and returns a vector of FileEntry items sorted in descending order by file size.
/// Errors are wrapped to satisfy `Send + 'static` and are returned only if the traversal itself fails catastrophically.
pub fn scan_files(start_path: &str) -> Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>> {
    let mut files = Vec::new();

    // Use automatic parallelism based on CPU cores
    for entry in WalkDir::new(start_path)
        .parallelism(Parallelism::RayonDefaultPool {
            busy_timeout: Duration::from_millis(100),
        })
        .into_iter()
        .filter_map(|e| e.ok())
    {
        // Check if it's a file
        let ft = entry.file_type();
        if ft.is_file() {
            if let Ok(metadata) = entry.metadata() {
                let path = entry.path();
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
            } else {
                // If metadata access fails, log and continue
                eprintln!("Failed to read metadata for {:?}", entry.path());
                continue;
            }
        }
    }

    files.sort_by(|a, b| b.size.cmp(&a.size));
    Ok(files)
}

/// Lists the contents of the directory at `start_path` (non-recursively) using jwalk.
pub fn list_directory(start_path: &str) -> Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>> {
    let mut entries = Vec::new();
    
    // Use WalkDir with max_depth = 1 to list only immediate children.
    for entry in WalkDir::new(start_path)
        .max_depth(1)
        .parallelism(Parallelism::Serial)
        .into_iter()
        .filter_map(|e| e.ok()) 
    {
        // Skip the directory itself.
        if entry.path() == Path::new(start_path) {
            continue;
        }
        
        // Process files and directories with metadata
        if let Ok(metadata) = entry.metadata() {
            let ft = entry.file_type();
            let is_file = ft.is_file();
            let is_dir = ft.is_dir();
            
            if is_file || is_dir {
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
        } else {
            // Handle metadata access failure
            return Err(Box::new(io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to get metadata for {:?}", entry.path()),
            )) as Box<dyn Error + Send + 'static>);
        }
    }
    
    // Optionally sort entries by name or by size.
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// Message types for progress reporting during a full storage scan
#[derive(Debug, Clone)]
pub enum ScanProgressMessage {
    FileScanned {
        size: u64,
    },
    ScanComplete {
        results: Vec<FileEntry>,
    },
}

/// Performs a full scan of the storage device, reporting progress via the progress channel.
/// This function is designed to be run in a background thread and will send progress updates
/// through the provided channel.
pub fn full_scan_with_progress(
    start_path: &str,
    _total_size: u64, // Not used directly but kept for API consistency
    progress_tx: Sender<ScanProgressMessage>,
) -> Result<(), Box<dyn Error + Send + 'static>> {
    let mut files = Vec::new();
    let progress_tx = Arc::new(progress_tx);

    for entry in WalkDir::new(start_path)
        .parallelism(Parallelism::RayonDefaultPool {
            busy_timeout: Duration::from_millis(100),
        })
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let ft = entry.file_type();
        if ft.is_file() {
            if let Ok(metadata) = entry.metadata() {
                let path = entry.path();
                let size = metadata.len();
                let name = path
                    .file_name()
                    .map(|os_str| os_str.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                
                // Send progress update
                let tx = Arc::clone(&progress_tx);
                let progress_msg = ScanProgressMessage::FileScanned { size };
                if let Err(e) = tx.blocking_send(progress_msg) {
                    eprintln!("Failed to send progress update: {}", e);
                }
                
                files.push(FileEntry {
                    name,
                    path: path.to_string_lossy().into_owned(),
                    size,
                });
            } else {
                // Log metadata access failure
                eprintln!("Failed to read metadata for {:?}", entry.path());
                continue;
            }
        }
    }

    // Sort files by size (largest first)
    files.sort_by(|a, b| b.size.cmp(&a.size));
    
    // Send completion message with results
    let complete_msg = ScanProgressMessage::ScanComplete { results: files };
    if let Err(e) = progress_tx.blocking_send(complete_msg) {
        eprintln!("Failed to send scan completion message: {}", e);
    }
    
    Ok(())
}