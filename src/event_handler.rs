use std::{error::Error, sync::mpsc, thread, time::Duration};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crate::{App, AppMode, PanelFocus};
use crate::platform::macos;
use crate::scanner::scan_files;
use tokio::sync::mpsc::Sender;

pub async fn process_event(
    app: &mut App,
    mode: &mut AppMode,
    async_tx: &Sender<Result<Vec<crate::scanner::FileEntry>, Box<dyn Error + Send + 'static>>>,
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
                                // Full scan optional.
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
                            _ => {}
                        }
                    },
                    AppMode::ConfirmEject(index) => {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                if let Some(device) = app.devices.get(*index) {
                                    match macos::eject_device(device) {
                                        Ok(()) => {
                                            let device_name = device.name.clone();
                                            app.devices.remove(*index);
                                            if app.devices.is_empty() {
                                                app.selected = 0;
                                            } else if app.selected >= app.devices.len() {
                                                app.selected = app.devices.len() - 1;
                                            }
                                            *mode = AppMode::Ejected(format!("Ejected Device: {} successfully", device_name));
                                        },
                                        Err(err) => {
                                            *mode = AppMode::Ejected(format!("Failed to eject {}: {}", device.name, err));
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
                        // Ignore key events while scanning.
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
        loop {
            let new_devices = crate::platform::macos::detect_storage_devices();
            if new_devices != old_devices {
                let _ = tx.send(new_devices.clone());
                old_devices = new_devices;
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}
