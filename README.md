# Lazy Storage Manager

Lazy Storage Manager is a modular terminal-based application written in Rust for managing HDD/SSD storage. Currently, it targets macOS but is designed with a modular architecture that makes it easy to extend support to Linux and Windows in the future.

The application features an interactive TUI built with [ratatui](https://crates.io/crates/ratatui) and [crossterm](https://crates.io/crates/crossterm). It provides key functionalities including:

- **Device management:**
  Displaying a list of storage devices and their details (e.g. name, mount point, total/free space, and vendor info). Devices that are ejectable (external drives) are marked with an eject icon.

- **File Listing & Scanning:**
  On startup, the application shows a quick (non‑recursive) directory listing of the selected device. The user can trigger a full deep scan of the storage device (using Shift‑S) which recursively scans all files, reports progress via a gauge, and then displays the files sorted by descending size.

- **File Operations:**
  Basic file operations (copy, move, delete) are supported through confirmation dialogs and are triggered via dedicated keys when the file list is focused.

- **Junk Scanning (Optional):**
  A separate module (`junk_scanner.rs`) is provided for scanning common junk directories (configurable by OS) to help identify unused or orphaned files. This feature leverages configuration files (e.g. TOML) to list known junk paths.

- **Help & Keyboard Shortcuts:**
  A detailed help overlay is available (triggered by the “?” key) to guide users with all available shortcuts and commands.

---

## Project Structure

The project is organized into the following modules:

- **`main.rs`**
  Initializes the application, sets up terminal I/O, spawns background tasks for device detection and file listing, and maintains the main UI/event loop. It uses `tokio` for asynchronous tasks and spawns long‑running file system scans using `spawn_blocking` to keep the UI responsive.

- **`ui.rs`**
  Contains all the TUI-related code. It is responsible for drawing the panels including the device list, device details/usage gauge (left panel), file and folder listings, and the scan progress gauge (right panel). The UI also supports help overlays and popup dialogs for confirmation.

- **`event_handler.rs`**
  Manages all key and event handling. It processes navigation keys (j/k, arrow keys), panel focus switches (Ctrl‑l/Ctrl‑h), refresh commands, ejection confirmations, file operation commands, and triggers both quick (non‑recursive) directory listings and full recursive scans.

- **`scanner.rs`**
  Implements the file system scanning logic. It provides two main functions:
  - `list_directory`: A quick, non‑recursive listing of the selected device’s root.
  - `scan_files_with_progress`: A full deep scan of a storage device that updates progress using atomic counters and returns a list of files sorted by size.

- **`junk_scanner.rs`**
  Contains logic for scanning known “junk” directories on the system. It loads configuration from a TOML file (located under the platform folder) and processes junk files by grouping them by folder. This module is useful for identifying orphaned data.

- **`macos.rs`**
  Provides macOS‑specific functionality. It uses the `sysinfo` crate to detect storage devices and leverages `diskutil` to extract extra device information (such as file system type, manufacturer, protocol) and for ejecting external devices.

- **`storage/` (optional)**
  This module (if used) could contain additional storage management logic or support for other operating systems.

- **Configuration Files**
  A configuration file (e.g., `junk_paths.toml`) is used by the junk scanner to define directories that are considered “junk” on each operating system. This file makes the tool customizable without changing code.

---

## Features

- **Modular Design:**
  Code is organized into separate modules for UI, event handling, scanning, and platform-specific implementations.

- **Responsive TUI:**
  The terminal-based interface remains responsive during long file scans by running heavy I/O operations in background tasks.

- **File System Scanning:**
  - Quick listing: Shows immediate (non‑recursive) files and folders.
  - Full scan: Recursively scans the entire device, tracking progress with a gauge and displaying results sorted by file size.

- **Device and File Operations:**
  Supports device refresh, ejection, as well as file-level operations (copy, move, delete) with confirmations.

- **Junk File Detection:**
  (Optional) A junk scanning mode that detects and aggregates junk files by folder using a configurable set of rules.

- **Help Overlay and Keyboard Shortcuts:**
  Provides an in‑app help screen that lists all available keyboard commands.

---

## Build Instructions

### Prerequisites

- **Rust:** Ensure you have the latest stable Rust toolchain installed. You can install it with [rustup](https://rustup.rs/).
- **Cargo:** The Rust build tool (included with rustup).

### Building

1. **Clone the repository:**
   ```bash
   git clone https://github.com/donjordano/lazysmg.git
   cd lazysng
   ```

2. **Clean and build the project:**
   ```bash
   cargo clean
   cargo build --release
   ```

3. **Run the application:**
   ```bash
   cargo run --release
   ```

   Alternatively, you can run in debug mode (slower but with more error information):
   ```bash
   cargo run
   ```

### Additional Setup for macOS

- The application uses macOS-specific commands (via `diskutil`) for ejecting devices and extracting storage information. Ensure that these command-line tools are available on your system.

- If you wish to modify junk scanning paths, edit the `junk_paths.toml` file located under the `src/platform/` directory.

---

## Usage

### Keyboard Shortcuts

- **General:**
  - `q` – Quit the application.
  - `?` – Toggle the help overlay.

- **Navigation:**
  - `j` / `k` – Move up/down in the device list (left panel) or file listing (right panel).
  - `Ctrl-l` / `Ctrl-h` – Switch focus between left and right panels.

- **Device Operations:**
  - `r` – Refresh the device list.
  - `e` – Eject the selected device (if ejectable).

- **File Listing and Scanning:**
  - `s` – Quick scan: update the non‑recursive file listing.
  - `S` (Shift + s) – Trigger a full deep scan of the selected device.
    The full scan shows progress in the bottom right gauge and, upon completion, updates the file listing (top right) with files sorted in descending order by size.

- **File Operations (when the right panel is focused):**
  - `d` – Delete a file (with confirmation).
  - `c` – Copy a file (with confirmation).
  - `m` – Move a file (with confirmation).

### Workflow

1. **Start the Application:**
   On launch, the app detects storage devices and displays a quick non‑recursive listing of the selected device’s files in the right panel.

2. **Switching Devices:**
   Use the left panel (j/k) to select a device. The quick listing updates automatically.

3. **Full Scan:**
   Press `S` (uppercase S) to initiate a deep full scan. The bottom right panel will show a gauge with scan progress. When the scan completes, the file listing in the top right panel will update with a full recursive listing sorted by file size.

4. **File Operations:**
   With the file list in focus, use keys such as `d`, `c`, or `m` to delete, copy, or move files, respectively. Confirm the operation when prompted.

5. **Help and Exit:**
   Press `?` to display the help overlay, and `q` to exit the application.

---

## Contributing

Contributions are welcome! Feel free to open issues or pull requests with suggestions, bug fixes, or new features. For major changes, please open an issue first to discuss your proposal.

---

## License

Distributed under the MIT License. See [LICENSE](LICENSE) for more information.