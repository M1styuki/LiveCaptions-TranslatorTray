//! One worker-owned storage thread will serialize settings, history, migration
//! and export. No storage object or dependency is initialized by the tray role.
pub mod actor;
pub mod atomic_file;
pub mod export;
pub mod history;
pub mod migrate;
pub mod settings_file;
pub mod timestamp;
