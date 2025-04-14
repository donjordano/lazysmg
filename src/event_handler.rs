use std::{error::Error, sync::mpsc, thread, time::Duration};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crate::{App, AppMode, PanelFocus, ScanProgress};
use crate::platform::macos;
use crate::scanner::{scan_files, full_scan_with_progress, ScanProgressMessage};
use tokio::sync::mpsc::Sender;

pub async fn process_event(
    app: &mut App,
    mode: &mut AppMode,
    async_tx: &Sender<Result<Vec<crate::scanner::FileEntry>, Box<dyn Error + Send + 'static>>>,
    progress_tx: &Sender<ScanProgressMessage>,
) -> Result<bool, Box<dyn Error>> {
    if event::poll(Duration::from_millis(200))? {
        if let Event::Key(key) = event::read()? {
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
                            KeyCode::Char('j') => {
                                app.next();
                            },
                            KeyCode::Char('k') => {
                                app.previous();
                            },
                            KeyCode::Char('r') => {
                                app.refresh();
                            },
                            KeyCode::Char('e') => {
                                if !app.devices.is_empty() && app.devices[app.selected].ejectable {
                                    *mode = AppMode::ConfirmEject(app.selected);
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
                            KeyCode::Char('S') => {
                                // Full device scan with progress tracking
                                if !app.devices.is_empty() {
                                    let device = &app.devices[app.selected];
                                    let mount = device.mount_point.clone();
                                    let total_size = device.total_space;
                                    
                                    // Set up progress tracking
                                    app.scan_progress = ScanProgress {
                                        total_bytes: total_size,
                                        scanned_bytes: 0,
                                        files_processed: 0,
                                        in_progress: true,
                                    };
                                    
                                    // Create a clone of the progress channel
                                    let progress_sender = progress_tx.clone();
                                    
                                    // Spawn the full scan task
                                    tokio::spawn(async move {
                                        let _ = tokio::task::spawn_blocking(move || {
                                            full_scan_with_progress(&mount, total_size, progress_sender)
                                        }).await;
                                    });
                                    
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
                                    let device_mount = device.mount_point.clone();
                                    
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
