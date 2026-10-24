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

pub use self::archive::*;
pub use self::catalog::*;
pub use self::detect::*;
pub use self::extract::*;
pub use self::install::*;
pub use self::manager::*;
pub use self::resolve::*;
pub use self::scrapers::*;
pub use self::types::*;
