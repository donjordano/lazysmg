use std::{
    collections::HashMap,
    error::Error,
    fs,
    path::PathBuf,
    time::Duration,
};
use expanduser::expanduser;
use jwalk::{WalkDir, Parallelism};
use serde::Deserialize;
use tokio::sync::mpsc::Sender;
use crate::scanner::{FileEntry, ScanProgressMessage};

#[derive(Debug, Deserialize)]
pub struct JunkPathsConfig {
    macos: JunkPathsSection,
    linux: JunkPathsSection,
    windows: JunkPathsSection,
}

#[derive(Debug, Deserialize)]
struct JunkPathsSection {
    paths: Vec<String>,
}

/// Results of a junk scan, grouped by directory
#[derive(Debug, Clone)]
pub struct JunkScanResults {
    pub folders: HashMap<String, FolderSummary>,
    pub total_size: u64,
    pub total_files: usize,
}

/// Summary information for a folder with junk files
#[derive(Debug, Clone)]
pub struct FolderSummary {
    pub path: String,
    pub files: Vec<FileEntry>,
    pub total_size: u64,
}

impl JunkScanResults {
    pub fn new() -> Self {
        JunkScanResults {
            folders: HashMap::new(),
            total_size: 0,
            total_files: 0,
        }
    }

    /// Add a file to the results, grouping by its parent folder
    pub fn add_file(&mut self, file: FileEntry) {
        // Extract parent folder path
        let path = PathBuf::from(&file.path);
        let parent_path = if let Some(parent) = path.parent() {
            parent.to_string_lossy().to_string()
        } else {
            // If no parent, use the path itself (unlikely)
            file.path.clone()
        };

        // Add file size to total
        self.total_size += file.size;
        self.total_files += 1;

        // Add or update folder summary
        let folder_summary = self.folders.entry(parent_path.clone()).or_insert_with(|| FolderSummary {
            path: parent_path,
            files: Vec::new(),
            total_size: 0,
        });

        folder_summary.total_size += file.size;
        folder_summary.files.push(file);
    }

    /// Sort folder summaries by size (largest first)
    pub fn sort_by_size(&mut self) {
        // Sort files within each folder
        for folder_summary in self.folders.values_mut() {
            folder_summary.files.sort_by(|a, b| b.size.cmp(&a.size));
        }
    }

    /// Convert results to a flat list of file entries sorted by size
    pub fn to_file_entries(&self) -> Vec<FileEntry> {
        let mut result = Vec::new();
        
        for folder in self.folders.values() {
            for file in &folder.files {
                result.push(file.clone());
            }
        }
        
        result.sort_by(|a, b| b.size.cmp(&a.size));
        result
    }
}

/// Load junk paths from the built-in TOML configuration file
pub fn load_junk_paths_config() -> Result<JunkPathsConfig, Box<dyn Error>> {
    let config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("platform")
        .join("junk_paths.toml");
    
    let content = fs::read_to_string(config_path)?;
    let config: JunkPathsConfig = toml::from_str(&content)?;
    
    Ok(config)
}

/// Get junk paths for the current OS, with expanded home directories
pub fn get_junk_paths_for_current_os() -> Result<Vec<String>, Box<dyn Error>> {
    let config = load_junk_paths_config()?;
    
    // Get paths for the current OS
    #[cfg(target_os = "macos")]
    let paths = config.macos.paths;
    
    #[cfg(target_os = "linux")]
    let paths = config.linux.paths;
    
    #[cfg(target_os = "windows")]
    let paths = config.windows.paths;
    
    // Expand paths (~ and environment variables)
    let expanded_paths = paths.iter()
        .filter_map(|path| {
            match expanduser(path) {
                Ok(expanded) => Some(expanded.to_string_lossy().to_string()),
                Err(_) => {
                    eprintln!("Failed to expand path: {}", path);
                    None
                }
            }
        })
        .collect();
    
    Ok(expanded_paths)
}

/// Scan system junk, using the junk_paths.toml configuration
/// Sends progress updates through the provided channel and returns the final results
pub async fn scan_system_junk(
    progress_tx: Sender<ScanProgressMessage>,
) -> Result<JunkScanResults, Box<dyn Error>> {
    let junk_paths = get_junk_paths_for_current_os()?;
    let mut results = JunkScanResults::new();
    
    // Scan each junk path
    for base_path in junk_paths {
        // Skip if path doesn't exist
        if !PathBuf::from(&base_path).exists() {
            continue;
        }
        
        // Walk directory
        for entry in WalkDir::new(&base_path)
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
                    
                    // Create file entry
                    let file_entry = FileEntry {
                        name,
                        path: path.to_string_lossy().into_owned(),
                        size,
                    };
                    
                    // Add file to results
                    results.add_file(file_entry.clone());
                    
                    // Send progress update
                    let progress_msg = ScanProgressMessage::FileScanned { 
                        size,
                        path: path.to_string_lossy().into_owned(),
                    };
                    
                    if let Err(e) = progress_tx.send(progress_msg).await {
                        eprintln!("Failed to send progress update: {}", e);
                    }
                }
            }
        }
    }
    
    // Sort results
    results.sort_by_size();
    
    // Send completion message
    let completion_msg = ScanProgressMessage::ScanComplete { 
        results: results.to_file_entries(),
        files_processed: results.total_files,
    };
    
    if let Err(e) = progress_tx.send(completion_msg).await {
        eprintln!("Failed to send scan completion message: {}", e);
    }
    
    Ok(results)
}