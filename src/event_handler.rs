use std::{error::Error, sync::mpsc, thread, time::Duration};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crate::{App, AppMode, PanelFocus, ScanProgress, FileOperation, ScanMode};
use crate::platform::{macos, junk_scanner};
use crate::scanner::{scan_files, full_scan_with_progress, ScanProgressMessage};
use crate::perform_file_operation;
use tokio::sync::mpsc::Sender;

pub async fn process_event(
    app: &mut App,
    mode: &mut AppMode,
    async_tx: &Sender<Result<Vec<crate::scanner::FileEntry>, Box<dyn Error + Send + 'static>>>,
    progress_tx: &Sender<ScanProgressMessage>,
) -> Result<bool, Box<dyn Error>> {
    if event::poll(Duration::from_millis(200))? {
        if let Event::Key(key) = event::read()? {
            // Global key handlers
            match key.code {
                // Toggle help screen
                KeyCode::Char('?') => {
                    app.show_help = !app.show_help;
                    return Ok(false);
                },
                _ => {}
            }

            // Handle panel switching with Ctrl-l and Ctrl-h.
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('l') => {
                        app.focus = PanelFocus::Right;
                    }
                    KeyCode::Char('h') => {
                        app.focus = PanelFocus::Left;
                    }
                    _ => {}
                }
            } else {
                // Process keys in Normal mode.
                match mode {
                    AppMode::Normal => {
                        match key.code {
                            KeyCode::Char('q') => return Ok(true),
                            KeyCode::Char('j') if app.focus == crate::PanelFocus::Left => {
                                app.next();
                            },
                            KeyCode::Char('k') if app.focus == crate::PanelFocus::Left => {
                                app.previous();
                            },
                            KeyCode::Char('j') | KeyCode::Down if app.focus == crate::PanelFocus::Right => {
                                app.next_file();
                            },
                            KeyCode::Char('k') | KeyCode::Up if app.focus == crate::PanelFocus::Right => {
                                app.previous_file();
                            },
                            KeyCode::Char('r') => {
                                app.refresh();
                            },
                            KeyCode::Char('e') => {
                                if !app.devices.is_empty() && app.devices[app.selected].ejectable {
                                    *mode = AppMode::ConfirmEject(app.selected);
                                }
                            },
                            // File operations when right panel is focused
                            KeyCode::Char('d') if app.focus == crate::PanelFocus::Right => {
                                if app.get_selected_file_entry().is_some() {
                                    *mode = AppMode::ConfirmFileOp {
                                        op_type: crate::FileOperation::Delete,
                                        file_index: app.selected_file_index,
                                        target_path: None,
                                    };
                                }
                            },
                            KeyCode::Char('c') if app.focus == crate::PanelFocus::Right => {
                                if let Some(file) = app.get_selected_file_entry() {
                                    // For now, set a dummy target path
                                    let target_path = format!("{}/copied_{}", app.devices[app.selected].mount_point,
                                        std::path::Path::new(&file.path).file_name().unwrap_or_default().to_string_lossy());
                                    *mode = AppMode::ConfirmFileOp {
                                        op_type: crate::FileOperation::Copy,
                                        file_index: app.selected_file_index,
                                        target_path: Some(target_path),
                                    };
                                }
                            },
                            KeyCode::Char('m') if app.focus == crate::PanelFocus::Right => {
                                if let Some(file) = app.get_selected_file_entry() {
                                    // For now, set a dummy target path
                                    let target_path = format!("{}/moved_{}", app.devices[app.selected].mount_point,
                                        std::path::Path::new(&file.path).file_name().unwrap_or_default().to_string_lossy());
                                    *mode = AppMode::ConfirmFileOp {
                                        op_type: crate::FileOperation::Move,
                                        file_index: app.selected_file_index,
                                        target_path: Some(target_path),
                                    };
                                }
                            },
                            KeyCode::Char('s') => {
                                // Regular scan (directory listing)
                                if !app.devices.is_empty() {
                                    let mount = app.devices[app.selected].mount_point.clone();
                                    let sender = async_tx.clone();
                                    tokio::spawn(async move {
                                        let result = tokio::task::spawn_blocking(move || scan_files(&mount))
                                            .await
                                            .unwrap_or_else(|e| Err(Box::new(e) as Box<dyn Error + Send + 'static>));
                                        let _ = sender.send(result).await;
                                    });
                                    *mode = AppMode::Scanning { device_index: app.selected, spinner_index: 0 };
                                }
                            },
                            KeyCode::Tab => {
                                // Toggle folder view in junk scan mode
                                if app.folder_summaries.is_some() && app.scan_mode == crate::ScanMode::JunkScan {
                                    app.folder_view_mode = !app.folder_view_mode;
                                    // Reset indices when switching views
                                    if app.folder_view_mode {
                                        app.selected_file_index = 0;
                                    } else {
                                        app.selected_folder_index = 0;
                                    }
                                    app.file_list_offset = 0;
                                }
                            },
                            KeyCode::Enter => {
                                // When in folder view, switch to file view showing files from selected folder
                                if app.folder_view_mode && app.folder_summaries.is_some() {
                                    app.folder_view_mode = false;
                                    // TODO: Filter files to show only those from selected folder
                                    app.selected_file_index = 0;
                                    app.file_list_offset = 0;
                                }
                            },
                            KeyCode::Char('S') => {
                                // Full device scan with progress tracking
                                if !app.devices.is_empty() {
                                    let device = &app.devices[app.selected];
                                    let mount = device.mount_point.clone();
                                    let total_size = device.total_space;
                                    let is_system_storage = !device.ejectable;

                                    // Reset folder view mode
                                    app.folder_view_mode = false;
                                    app.selected_folder_index = 0;

                                    // Set up progress tracking
                                    app.scan_progress = ScanProgress {
                                        total_bytes: total_size,
                                        scanned_bytes: 0,
                                        files_processed: 0,
                                        in_progress: true,
                                        current_file: None,
                                    };

                                    // Create a clone of the progress channel
                                    let progress_sender = progress_tx.clone();

                                    // Different scan types based on device type
                                    if is_system_storage {
                                        // For system storage, scan for junk files
                                        app.scan_mode = ScanMode::JunkScan;
                                        
                                        // Spawn the junk scan task
                                        let progress_clone = progress_sender.clone();
                                        tokio::spawn(async move {
                                            let _ = junk_scanner::scan_system_junk(progress_clone).await;
                                        });
                                    } else {
                                        // For external/ejectable devices, do a full scan
                                        app.scan_mode = ScanMode::FullScan;
                                        
                                        // Spawn the full scan task
                                        tokio::spawn(async move {
                                            let _ = tokio::task::spawn_blocking(move || {
                                                full_scan_with_progress(&mount, total_size, progress_sender)
                                            }).await;
                                        });
                                    }

                                    *mode = AppMode::FullScan {
                                        device_index: app.selected,
                                        spinner_index: 0
                                    };
                                }
                            },
                            _ => {}
                        }
                    },
                    AppMode::ConfirmEject(index) => {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                if let Some(device) = app.devices.get(*index) {
                                    // Clone the device info we need before borrowing
                                    let device_name = device.name.clone();
                                    // Unused variable - remove it
                                    // let device_mount = device.mount_point.clone();

                                    match macos::eject_device(device) {
                                        Ok(()) => {
                                            // Use refresh instead of manual removal to ensure consistency
                                            app.refresh();
                                            // Clear any file listings for the ejected device
                                            app.file_entries = None;
                                            app.full_scan_results = None;
                                            *mode = AppMode::Ejected(format!("Ejected Device: {} successfully", device_name));
                                        },
                                        Err(err) => {
                                            // Still refresh in case of partial ejection
                                            app.refresh();
                                            *mode = AppMode::Ejected(format!("Failed to eject {}: {}", device_name, err));
                                        },
                                    }
                                } else {
                                    *mode = AppMode::Normal;
                                }
                            },
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                *mode = AppMode::Normal;
                            },
                            _ => {}
                        }
                    },
                    AppMode::Ejected(_) => {
                        *mode = AppMode::Normal;
                    },
                    AppMode::ConfirmFileOp { op_type, file_index, target_path } => {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                // Clone needed values from the operation
                                let op_type_clone = op_type.clone();
                                let file_index_clone = *file_index;
                                let target_path_clone = target_path.clone();

                                // Get the source file path
                                if let Some(file) = app.get_selected_file_entry() {
                                    let source_path = file.path.clone();

                                    // Perform the file operation
                                    match perform_file_operation(
                                        &op_type_clone,
                                        &source_path,
                                        target_path_clone.as_deref()
                                    ) {
                                        Ok(result) => {
                                            // Refresh file list after the operation
                                            app.selected_file_index = 0;

                                            if let Some(ref mut entries) = app.full_scan_results {
                                                // For deletion, remove from the list
                                                if let FileOperation::Delete = op_type_clone {
                                                    if file_index_clone < entries.len() {
                                                        entries.remove(file_index_clone);
                                                    }
                                                }
                                            }

                                            // Trigger a refresh of the regular file listing as well
                                            app.file_entries = None;
                                            app.scanning = true;
                                            let mount = app.devices[app.selected].mount_point.clone();
                                            let sender = async_tx.clone();
                                            tokio::spawn(async move {
                                                let result = tokio::task::spawn_blocking(move ||
                                                    crate::scanner::list_directory(&mount)
                                                ).await.unwrap_or_else(|e|
                                                    Err(Box::new(e) as Box<dyn Error + Send + 'static>)
                                                );
                                                let _ = sender.send(result).await;
                                            });

                                            *mode = AppMode::Ejected(format!("File operation result: {}", result));
                                        },
                                        Err(err) => {
                                            *mode = AppMode::Ejected(format!("Operation failed: {}", err));
                                        }
                                    }
                                } else {
                                    *mode = AppMode::Normal;
                                }
                            },
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                *mode = AppMode::Normal;
                            },
                            _ => {}
                        }
                    },
                    AppMode::Scanning { .. } => {
                        // Allow quitting or canceling during regular scan
                        match key.code {
                            KeyCode::Char('q') => {
                                return Ok(true);
                            },
                            KeyCode::Char('c') => {
                                app.scanning = false;
                                *mode = AppMode::Normal;
                            },
                            _ => {}
                        }
                    },
                    AppMode::FullScan { .. } => {
                        match key.code {
                            // Allow quitting during full scan
                            KeyCode::Char('q') => {
                                return Ok(true);
                            },
                            // Cancel the full scan
                            KeyCode::Char('c') => {
                                app.scan_progress.in_progress = false;
                                *mode = AppMode::Normal;
                            },
                            _ => {}
                        }
                    },
                }
            }
        }
    }
    Ok(false)
}

pub fn start_device_listener(tx: mpsc::Sender<Vec<crate::platform::macos::StorageDevice>>) {
    thread::spawn(move || {
        let mut old_devices = crate::platform::macos::detect_storage_devices();
        let mut last_check = std::time::Instant::now();

        loop {
            // Always check if we have an ejection event
            let new_devices = crate::platform::macos::detect_storage_devices();

            // Send updated devices if there's a change or after a full refresh interval
            let time_since_refresh = last_check.elapsed();
            if new_devices != old_devices || time_since_refresh.as_secs() >= 5 {
                if let Err(e) = tx.send(new_devices.clone()) {
                    eprintln!("Error sending device update: {}", e);
                    break;
                }
                old_devices = new_devices;
                last_check = std::time::Instant::now();
            }

            thread::sleep(Duration::from_millis(500));
        }
    });
}
