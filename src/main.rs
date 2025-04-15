mod ui;
mod event_handler;
mod platform;
mod scanner;
mod storage; // if needed

use std::{
    error::Error,
    sync::mpsc,
    time::Duration,
};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use ui::draw_app;
use event_handler::process_event;
use platform::macos::{detect_storage_devices, StorageDevice};
use scanner::{FileEntry, list_directory, ScanProgressMessage};

/// Which panel is focused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelFocus {
    Left,
    Right,
}

/// Application mode. We keep only Normal (with a possible scanning flag).
#[derive(Debug, Clone)]
pub enum AppMode {
    Normal,
    ConfirmEject(usize),
    Ejected(String),
    Scanning { device_index: usize, spinner_index: usize },
    FullScan { device_index: usize, spinner_index: usize },
    ConfirmFileOp { 
        op_type: FileOperation, 
        file_index: usize,
        target_path: Option<String> // For copy/move operations
    },
}

#[derive(Debug, Clone)]
pub enum FileOperation {
    Copy,
    Move,
    Delete,
}

/// Different scanning modes for the application
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanMode {
    /// Regular full scan of a device (external or ejectable)
    FullScan,
    /// Junk scan mode (system storage only)
    JunkScan,
}

/// Summary of a folder containing junk files
#[derive(Debug, Clone)]
pub struct FolderSummary {
    pub path: String,
    pub total_size: u64,
    pub file_count: usize,
}

/// Tracks progress during a full storage scan
#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub total_bytes: u64,         // Total size of the storage device
    pub scanned_bytes: u64,       // Total bytes scanned so far
    pub files_processed: u64,     // Number of files processed
    pub in_progress: bool,        // Whether a full scan is in progress
    pub current_file: Option<String>, // Currently being processed file
}

/// Main application state.
#[derive(Debug)]
pub struct App {
    pub devices: Vec<StorageDevice>,
    pub selected: usize,
    pub file_entries: Option<Vec<FileEntry>>, // current directory listing for the selected device
    pub scanning: bool,                        // whether a directory listing is in progress
    pub focus: PanelFocus,
    pub full_scan_results: Option<Vec<FileEntry>>, // results from a full device scan
    pub scan_progress: ScanProgress,               // tracks progress during full scan
    pub selected_file_index: usize,                // currently selected file in the list
    pub clipboard: Option<(String, FileOperation)>, // stores path and operation type for copy/move
    pub file_list_offset: usize,                   // scrolling offset for file list
    pub device_results: std::collections::HashMap<String, Vec<FileEntry>>, // results per device
    pub show_help: bool,                          // whether to show the help overlay
    pub scan_mode: ScanMode,                      // current scan mode
    pub folder_summaries: Option<Vec<FolderSummary>>, // folder summaries for junk scan
    pub selected_folder_index: usize,             // selected folder in junk scan view
    pub folder_view_mode: bool,                   // whether we're viewing folders or files
}

impl App {
    pub fn new(devices: Vec<StorageDevice>) -> App {
        App {
            devices,
            selected: 0,
            file_entries: None,
            scanning: false,
            focus: PanelFocus::Left,
            full_scan_results: None,
            scan_progress: ScanProgress {
                total_bytes: 0,
                scanned_bytes: 0,
                files_processed: 0,
                in_progress: false,
                current_file: None,
            },
            selected_file_index: 0,
            clipboard: None,
            file_list_offset: 0,
            device_results: std::collections::HashMap::new(),
            show_help: false,
            scan_mode: ScanMode::FullScan,
            folder_summaries: None,
            selected_folder_index: 0,
            folder_view_mode: false,
        }
    }

    pub fn next(&mut self) {
        if !self.devices.is_empty() {
            self.selected = (self.selected + 1) % self.devices.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.devices.is_empty() {
            if self.selected == 0 {
                self.selected = self.devices.len() - 1;
            } else {
                self.selected -= 1;
            }
        }
    }

    pub fn refresh(&mut self) {
        self.devices = detect_storage_devices();
        if self.devices.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.devices.len() {
            self.selected = self.devices.len() - 1;
        }
    }
    
    pub fn next_file(&mut self) {
        let max_index = if let Some(ref entries) = self.full_scan_results {
            entries.len().saturating_sub(1)
        } else if let Some(ref entries) = self.file_entries {
            entries.len().saturating_sub(1)
        } else {
            0
        };
        
        if max_index > 0 && self.selected_file_index < max_index {
            self.selected_file_index += 1;
            
            // Adjust scroll offset if needed (maintain visibility)
            // Assuming we show ~15 items at once
            if self.selected_file_index >= self.file_list_offset + 14 {
                self.file_list_offset = self.selected_file_index - 14;
            }
        }
    }
    
    pub fn previous_file(&mut self) {
        if self.selected_file_index > 0 {
            self.selected_file_index -= 1;
            
            // Adjust scroll offset if needed (maintain visibility)
            if self.selected_file_index < self.file_list_offset {
                self.file_list_offset = self.selected_file_index;
            }
        }
    }
    
    pub fn get_selected_file_entry(&self) -> Option<&FileEntry> {
        if let Some(ref entries) = self.full_scan_results {
            if self.selected_file_index < entries.len() {
                return Some(&entries[self.selected_file_index]);
            }
        } else if let Some(ref entries) = self.file_entries {
            if self.selected_file_index < entries.len() {
                return Some(&entries[self.selected_file_index]);
            }
        }
        None
    }
}

/// Performs file operations
pub fn perform_file_operation(
    op_type: &FileOperation, 
    source_path: &str, 
    target_path: Option<&str>
) -> Result<String, Box<dyn std::error::Error>> {
    use std::fs;
    use std::path::Path;
    
    match op_type {
        FileOperation::Copy => {
            if let Some(target) = target_path {
                let source_path = Path::new(source_path);
                let target_path = Path::new(target);
                
                // Create parent directory if it doesn't exist
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                
                // Perform the copy
                fs::copy(source_path, target_path)?;
                Ok(format!("Copied {} to {}", source_path.display(), target_path.display()))
            } else {
                Err("Target path not provided for copy operation".into())
            }
        },
        FileOperation::Move => {
            if let Some(target) = target_path {
                let source_path = Path::new(source_path);
                let target_path = Path::new(target);
                
                // Create parent directory if it doesn't exist
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                
                // Perform the move
                fs::rename(source_path, target_path)?;
                Ok(format!("Moved {} to {}", source_path.display(), target_path.display()))
            } else {
                Err("Target path not provided for move operation".into())
            }
        },
        FileOperation::Delete => {
            let path = Path::new(source_path);
            if path.is_dir() {
                fs::remove_dir_all(path)?;
                Ok(format!("Deleted directory: {}", path.display()))
            } else {
                fs::remove_file(path)?;
                Ok(format!("Deleted file: {}", path.display()))
            }
        },
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize terminal.
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create an mpsc channel for device updates.
    let (device_tx, device_rx) = mpsc::channel();
    event_handler::start_device_listener(device_tx);

    // Tokio mpsc channel for async directory listings.
    let (scan_tx, mut scan_rx) =
        tokio::sync::mpsc::channel::<Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>>>(1);
        
    // Channel for full scan progress updates
    let (progress_tx, mut progress_rx) = 
        tokio::sync::mpsc::channel::<scanner::ScanProgressMessage>(100);

    let devices = detect_storage_devices();
    let mut app = App::new(devices);
    let mut mode = AppMode::Normal;
    let spinner_chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    // When the app starts, if there is at least one device, trigger a directory listing for it.
    let mut last_selected = app.selected;
    if !app.devices.is_empty() {
        let mount = app.devices[app.selected].mount_point.clone();
        let sender = scan_tx.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || list_directory(&mount))
                .await
                .unwrap_or_else(|e| Err(Box::new(e) as Box<dyn Error + Send + 'static>));
            let _ = sender.send(result).await;
        });
        app.scanning = true;
        mode = AppMode::Scanning { device_index: app.selected, spinner_index: 0 };
    }

    loop {
        // Update device list from listener.
        if let Ok(new_devices) = device_rx.try_recv() {
            // Store previous selection info before updating device list
            let prev_selected = if !app.devices.is_empty() {
                Some(app.devices[app.selected].mount_point.clone())
            } else {
                None
            };
            
            // Update the device list
            app.devices = new_devices;
            
            // Update selection
            if app.devices.is_empty() {
                app.selected = 0;
                app.file_entries = None;
                app.full_scan_results = None;
            } else {
                // Try to maintain the same device selection if possible
                if let Some(prev_mount) = prev_selected {
                    if let Some(index) = app.devices.iter().position(|dev| dev.mount_point == prev_mount) {
                        app.selected = index;
                    } else {
                        // Previous device not found, reset selection and clear file entries
                        app.selected = 0;
                        app.file_entries = None;
                        app.full_scan_results = None;
                        // Trigger a directory listing for the new device
                        mode = AppMode::Scanning { device_index: app.selected, spinner_index: 0 };
                        last_selected = app.selected;
                        
                        // Start scan for the new selection
                        let mount = app.devices[app.selected].mount_point.clone();
                        let sender = scan_tx.clone();
                        tokio::spawn(async move {
                            let result = tokio::task::spawn_blocking(move || list_directory(&mount))
                                .await
                                .unwrap_or_else(|e| Err(Box::new(e) as Box<dyn Error + Send + 'static>));
                            let _ = sender.send(result).await;
                        });
                        app.scanning = true;
                    }
                } else if app.selected >= app.devices.len() {
                    app.selected = app.devices.len() - 1;
                    app.file_entries = None;
                    app.full_scan_results = None;
                }
            }
        }

        // When in Normal mode, check if the selection changed.
        if let AppMode::Normal = mode {
            if !app.devices.is_empty() && app.selected != last_selected {
                // A new device was selected
                app.selected_file_index = 0;   // Reset selection
                app.file_list_offset = 0;      // Reset scroll
                
                // Clear full scan results when switching devices
                app.full_scan_results = None;
                
                // Get current device ID
                let device_id = &app.devices[app.selected].name;
                
                // First check if we have full scan results for this device
                let has_full_scan = app.device_results.contains_key(device_id);
                
                if has_full_scan {
                    // Use the cached full scan results
                    if let Some(entries) = app.device_results.get(device_id) {
                        app.file_entries = Some(entries.clone());
                        app.full_scan_results = Some(entries.clone());
                    }
                } else {
                    // No full scan results, do a regular directory listing
                    app.scanning = true;
                    app.file_entries = None;
                    
                    let mount = app.devices[app.selected].mount_point.clone();
                    let sender = scan_tx.clone();
                    tokio::spawn(async move {
                        let result = tokio::task::spawn_blocking(move || list_directory(&mount))
                            .await
                            .unwrap_or_else(|e| Err(Box::new(e) as Box<dyn Error + Send + 'static>));
                        let _ = sender.send(result).await;
                    });
                    
                    // Update mode to scanning
                    mode = AppMode::Scanning { device_index: app.selected, spinner_index: 0 };
                }
                
                // Update last_selected.
                last_selected = app.selected;
            }
        }

        // In Scanning mode, update spinner and attempt to receive the file listing.
        if let AppMode::Scanning { ref mut spinner_index, .. } = mode {
            *spinner_index = (*spinner_index + 1) % spinner_chars.len();
            if let Ok(result) = scan_rx.try_recv() {
                match result {
                    Ok(file_entries) => {
                        // Store in device cache if we have a device selected
                        if !app.devices.is_empty() {
                            let device_id = app.devices[app.selected].name.clone();
                            app.device_results.insert(device_id, file_entries.clone());
                        }
                        
                        app.file_entries = Some(file_entries);
                        app.scanning = false;
                        mode = AppMode::Normal;
                    }
                    Err(e) => {
                        mode = AppMode::Ejected(format!("Scan failed: {}", e));
                        app.scanning = false;
                    }
                }
            }
        }
        
        // In FullScan mode, update spinner and check for progress updates
        if let AppMode::FullScan { ref mut spinner_index, .. } = mode {
            *spinner_index = (*spinner_index + 1) % spinner_chars.len();
            
            // Check for progress updates
            while let Ok(progress_msg) = progress_rx.try_recv() {
                match progress_msg {
                    ScanProgressMessage::FileScanned { size, path } => {
                        app.scan_progress.scanned_bytes += size;
                        app.scan_progress.files_processed += 1;
                        app.scan_progress.current_file = Some(path);
                    },
                    ScanProgressMessage::ScanComplete { results, files_processed } => {
                        // Store full scan results in both places
                        app.full_scan_results = Some(results.clone());
                        
                        // Also store in device cache if device is available
                        if !app.devices.is_empty() {
                            let device_id = app.devices[app.selected].name.clone();
                            app.device_results.insert(device_id, results);
                        }
                        
                        app.scan_progress.in_progress = false;
                        app.scan_progress.files_processed = files_processed as u64;
                        app.scan_progress.current_file = None;
                        app.folder_summaries = None; // No folder summaries for regular scans
                        mode = AppMode::Normal;
                    },
                    ScanProgressMessage::JunkScanComplete { results, files_processed, folder_summaries } => {
                        // Store full scan results in both places
                        app.full_scan_results = Some(results.clone());
                        
                        // Convert folder summaries to a format we can store
                        let summaries = folder_summaries
                            .into_iter()
                            .map(|(path, size, count)| FolderSummary {
                                path,
                                total_size: size,
                                file_count: count,
                            })
                            .collect();
                        
                        app.folder_summaries = Some(summaries);
                        
                        // Also store in device cache if device is available
                        if !app.devices.is_empty() {
                            let device_id = app.devices[app.selected].name.clone();
                            app.device_results.insert(device_id, results);
                        }
                        
                        app.scan_progress.in_progress = false;
                        app.scan_progress.files_processed = files_processed as u64;
                        app.scan_progress.current_file = None;
                        app.scan_mode = ScanMode::JunkScan;
                        mode = AppMode::Normal;
                    }
                }
            }
        }

        // Draw UI.
        draw_app(&mut terminal, &app, &mode, &spinner_chars)?;

        // Process key events.
        if process_event(&mut app, &mut mode, &scan_tx, &progress_tx).await? {
            break;
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Create a short delay to allow any in-progress tasks to complete gracefully
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Close the channels explicitly to prevent "channel closed" errors
    drop(scan_tx);
    drop(progress_tx);
    
    // Clean up terminal state
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    
    // Return success
    Ok(())
}
