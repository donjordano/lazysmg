use sysinfo::{DiskExt, System, SystemExt};

#[derive(Debug)]
pub struct StorageDevice {
    pub name: String,
    pub total_space: u64,
    pub available_space: u64,
    pub mount_point: String,
}

/// Detects storage devices (local and mounted) on macOS using the sysinfo crate.
pub fn detect_storage_devices() -> Vec<StorageDevice> {
    let mut sys = System::new_all();
    // Refresh disk information.
    sys.refresh_disks_list();
    sys.refresh_disks();

    sys.disks().iter().map(|disk| {
        StorageDevice {
            name: disk.name().to_string_lossy().into(),
            total_space: disk.total_space(),
            available_space: disk.available_space(),
            mount_point: disk.mount_point().to_string_lossy().into(),
        }
    }).collect()
}
