use sysinfo::{DiskExt, System, SystemExt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDevice {
    pub name: String,
    pub total_space: u64,
    pub available_space: u64,
    pub mount_point: String,
    pub ejectable: bool,
}

/// Detects storage devices (local and mounted) on macOS using the sysinfo crate.
pub fn detect_storage_devices() -> Vec<StorageDevice> {
    let mut sys = System::new_all();
    sys.refresh_disks_list();
    sys.refresh_disks();

    sys.disks().iter().map(|disk| {
        let mount_str = disk.mount_point().to_string_lossy().to_string();
        StorageDevice {
            name: disk.name().to_string_lossy().to_string(),
            total_space: disk.total_space(),
            available_space: disk.available_space(),
            mount_point: mount_str.clone(),
            // Consider devices mounted under "/Volumes/" as ejectable.
            ejectable: mount_str.starts_with("/Volumes/"),
        }
    }).collect()
}

/// Ejects a storage device on macOS by invoking "diskutil eject <mount_point>".
/// Returns Ok(()) if successful, or an error if the command fails.
pub fn eject_device(device: &StorageDevice) -> Result<(), Box<dyn std::error::Error>> {
    use std::process::Command;
    let output = Command::new("diskutil")
        .arg("eject")
        .arg(&device.mount_point)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "diskutil error: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}
