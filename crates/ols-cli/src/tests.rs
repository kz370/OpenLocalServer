use crate::ctx::explorer_path;
use std::path::{Path, PathBuf};

/// Explorer passes a shared folder as `UNC\server\share\path`, which is why the
/// right-click menu refused every folder on a network drive.
#[test]
fn a_unc_path_from_explorer_is_rewritten_to_a_real_one() {
    assert_eq!(
        explorer_path(Path::new(r"UNC\Kz370s\g\Downloads")),
        PathBuf::from(r"\\Kz370s\g\Downloads")
    );
    assert_eq!(
        explorer_path(Path::new("UNC/Kz370s/g/Downloads")),
        PathBuf::from(r"\\Kz370s\g\Downloads")
    );
}

#[test]
fn ordinary_paths_are_left_alone() {
    for p in [
        r"C:\Users\you\Sites\shop",
        r"\\server\share\folder",
        r"\\?\C:\very\long\path",
        r"relative\folder",
        r"\\?\UNC\server\share\folder",
    ] {
        assert_eq!(explorer_path(Path::new(p)), PathBuf::from(p), "{p}");
    }
    // A local folder whose name merely starts with the letters is not a share.
    assert_eq!(
        explorer_path(Path::new(r"C:\UNC\folder")),
        PathBuf::from(r"C:\UNC\folder")
    );
}
