use std::{
  error::Error,
  sync::mpsc,
  thread,
  time::Duration,
};
use crossterm::event::{self, Event, KeyCode};
use crate::{App, AppMode};
use crate::platform::macos;
use crate::scanner::scan_files;

pub fn process_event(
  app: &mut App,
  mode: &mut AppMode,
  scan_rx: &mut Option<mpsc::Receiver<Result<Vec<crate::scanner::FileEntry>, Box<dyn Error + Send + 'static>>>>,
) -> Result<bool, Box<dyn Error>> {
  if event::poll(Duration::from_millis(200))? {
      if let Event::Key(key) = event::read()? {
          match mode {
              AppMode::Normal => {
                  match key.code {
                      KeyCode::Char('q') => return Ok(true),
                      KeyCode::Char('j') => { app.next(); },
                      KeyCode::Char('k') => { app.previous(); },
                      KeyCode::Char('r') => { app.refresh(); },
                      KeyCode::Char('e') => {
                          if !app.devices.is_empty() && app.devices[app.selected].ejectable {
                              *mode = AppMode::ConfirmEject(app.selected);
                          }
                      },
                      KeyCode::Char('s') => {
                          if !app.devices.is_empty() {
                              let device = &app.devices[app.selected];
                              let mount = device.mount_point.clone();
                              let (tx_scan, rx_scan) = mpsc::channel();
                              thread::spawn(move || {
                                  let result = scan_files(&mount);
                                  let _ = tx_scan.send(result);
                              });
                              *mode = AppMode::Scanning { device_index: app.selected, spinner_index: 0 };
                              *scan_rx = Some(rx_scan);
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
                                  }
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
                  // Ignore events during scanning.
              },
              AppMode::FileList { selected, .. } => {
                match key.code {
                    KeyCode::Char('j') => *selected += 1,
                    KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                    KeyCode::Char('b') => { *mode = AppMode::Normal; },
                    _ => {}
                }
            },
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
