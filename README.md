# Lazy Storage Manager

Lazy Storage Manager is a modular Rust application designed to manage HDD and SSD storage. The application features a TUI built with ratatui and crossterm and currently includes macOS-specific implementations with an architecture that can be extended to support other operating systems.

## Features

- **TUI Interface:** Interactive terminal UI using ratatui and crossterm.
- **Modular Design:** Separate modules for OS-specific implementations and storage management.
- **Extensible:** Easily add support for additional operating systems and storage features.

## Project Structure


## Getting Started

### Prerequisites

- [Rust toolchain](https://rustup.rs/)

### Building and Running

Clone the repository, navigate to the project directory, and run:

```bash
cargo build --release
cargo run --release


---

## 5. Next Steps

- **Test Your Application:** Run `cargo run` to see the TUI in action and test the basic structure.
- **Implement Functionality:** Gradually add your specific logic in the `platform` and `storage` modules.
- **Expand the TUI:** Enhance the interface using more widgets and dynamic layouts provided by ratatui.

Following these steps will help you transform the basic Cargo project into a modular and TUI-enabled application built around your design for **lazystoragemanager**. Feel free to adjust and expand as your project requirements evolve!
