pub mod platform;
pub mod storage;
pub mod scanner;

// Re-export the scanner module for use in other modules
pub use scanner::{FileEntry, ScanProgressMessage};
