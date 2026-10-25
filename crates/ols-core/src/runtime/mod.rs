pub mod archive;
pub mod catalog;
pub mod detect;
pub mod extract;
pub mod install;
pub mod manager;
pub mod resolve;
pub mod scrapers;
pub mod types;

#[cfg(test)]
mod tests;

pub use self::detect::detect_system_install;
pub use self::manager::*;
pub use self::types::*;
