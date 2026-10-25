pub mod manage;
pub mod skip;

pub(crate) use self::manage::move_to_excluded_dir;
pub use self::skip::DeletedItem;

#[cfg(test)]
pub(crate) use self::skip::{same_skip_entry, skip_matches, SkipEntry};
#[cfg(not(test))]
pub(crate) use self::skip::{skip_matches, SkipEntry};
