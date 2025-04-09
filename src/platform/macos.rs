use sysinfo::{DiskExt, System, SystemExt};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDevice {
    pub name: String,
    pub total_space: u64,
    pub available_space: u64,
    pub mount_point: String,
    pub ejectable: bool,
    pub vendor_info: Option<String>,
}

/// Detects storage devices (local and mounted) on macOS using the sysinfo crate.
/// For each disk, we additionally run "diskutil info <mount_point>" and attempt to extract:
/// - File System Personality (FS type)
/// - Device / Media Name (Manufacturer)
/// - Protocol
pub fn detect_storage_devices() -> Vec<StorageDevice> {
    let mut sys = System::new_all();
    sys.refresh_disks_list();
    sys.refresh_disks();

    sys.disks().iter().map(|disk| {
        let mount_str = disk.mount_point().to_string_lossy().to_string();
        // Consider device ejectable if mount point starts with "/Volumes/"
        let ejectable = mount_str.starts_with("/Volumes/");

        // Try to gather extra info using "diskutil info"
        let vendor_info = {
            let output = Command::new("diskutil")
                .arg("info")
                .arg(&mount_str)
                .output();

            if let Ok(output) = output {
                let info_str = String::from_utf8_lossy(&output.stdout);
                let mut media = None;
                let mut protocol = None;
                let mut fs_type = None;
                for line in info_str.lines() {
                    if line.contains("Device / Media Name:") {
                        media = line.split(':').nth(1)
                                    .map(|s| s.trim().to_string());
                    } else if line.contains("Protocol:") {
                        protocol = line.split(':').nth(1)
                                       .map(|s| s.trim().to_string());
                    } else if line.contains("File System Personality:") {
                        fs_type = line.split(':').nth(1)
                                     .map(|s| s.trim().to_string());
                    }
                }
                let mut info_vec = Vec::new();
                if let Some(fs) = fs_type {
                    info_vec.push(format!("FS: {}", fs));
                }
                if let Some(manu) = media {
                    info_vec.push(format!("Manufacturer: {}", manu));
                }
                if let Some(proto) = protocol {
                    info_vec.push(format!("Protocol: {}", proto));
                }
                if !info_vec.is_empty() {
                    Some(info_vec.join(", "))
                } else {
                    None
                }
            } else {
                None
            }
        };

        StorageDevice {
            name: disk.name().to_string_lossy().to_string(),
            total_space: disk.total_space(),
            available_space: disk.available_space(),
            mount_point: mount_str,
            ejectable,
            vendor_info,
        }
    }).collect()
}

/// Ejects a storage device on macOS by invoking "diskutil eject <mount_point>".
/// Returns Ok(()) if the command succeeds; otherwise returns an error.
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
        ).into())
    }
}
