use crate::{config, GpsAnalyzerError};
use std::path::{Path, PathBuf};

fn stored_path_for(gpx_dir: &Path, input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    gpx_dir.join(format!("{stem}.gpx"))
}

/// True when both paths resolve to the same existing file. A path that cannot be canonicalized
/// (e.g. the destination does not exist yet) is treated as distinct.
fn same_existing_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Copy `input` into `gpx_dir`, keyed by its file stem (`{stem}.gpx`), returning the managed path.
/// Skips the copy when `input` already resolves to its store location (e.g. reindex/replay).
pub fn store_gpx_in(gpx_dir: &Path, input: &Path) -> Result<PathBuf, GpsAnalyzerError> {
    let dest = stored_path_for(gpx_dir, input);
    if same_existing_file(input, &dest) {
        return Ok(dest);
    }
    std::fs::create_dir_all(gpx_dir)?;
    std::fs::copy(input, &dest)?;
    Ok(dest)
}

/// Enumerate the `*.gpx` files in `gpx_dir`, sorted by path. Returns an empty list when the
/// directory does not exist.
pub fn gpx_files_in(gpx_dir: &Path) -> Result<Vec<PathBuf>, GpsAnalyzerError> {
    if !gpx_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(gpx_dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|ext| ext.eq_ignore_ascii_case("gpx"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    Ok(files)
}

/// Copy `input` into the GPX store at `config::gpx_dir()`.
pub fn store_gpx(input: &Path) -> Result<PathBuf, GpsAnalyzerError> {
    store_gpx_in(&config::gpx_dir(), input)
}

/// Enumerate the GPX files in the store at `config::gpx_dir()`.
pub fn stored_gpx_files() -> Result<Vec<PathBuf>, GpsAnalyzerError> {
    gpx_files_in(&config::gpx_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, contents: &str) {
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn test_store_gpx_copies_into_store_keyed_by_stem() {
        let src_dir = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let input = src_dir.path().join("my-ride.gpx");
        write_file(&input, "<gpx/>");

        let dest = store_gpx_in(store.path(), &input).unwrap();

        assert_eq!(dest, store.path().join("my-ride.gpx"));
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "<gpx/>");
    }

    #[test]
    fn test_store_gpx_already_in_store_is_noop() {
        let store = tempfile::tempdir().unwrap();
        let input = store.path().join("ride.gpx");
        write_file(&input, "original");

        let dest = store_gpx_in(store.path(), &input).unwrap();

        assert_eq!(dest, input);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "original");
    }

    #[test]
    fn test_store_gpx_overwrites_same_stem() {
        let src_dir = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let first = src_dir.path().join("ride.gpx");
        write_file(&first, "first");
        store_gpx_in(store.path(), &first).unwrap();

        let second = src_dir.path().join("nested");
        std::fs::create_dir_all(&second).unwrap();
        let second = second.join("ride.gpx");
        write_file(&second, "second");
        let dest = store_gpx_in(store.path(), &second).unwrap();

        assert_eq!(dest, store.path().join("ride.gpx"));
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "second");
    }

    #[test]
    fn test_gpx_files_in_missing_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(gpx_files_in(&missing).unwrap().is_empty());
    }

    #[test]
    fn test_store_gpx_and_stored_gpx_files_resolve_config_dir() {
        let home = tempfile::tempdir().unwrap();
        let src_dir = tempfile::tempdir().unwrap();
        let input = src_dir.path().join("wrapped-ride.gpx");
        write_file(&input, "<gpx/>");

        // Point the my-watts home at a temp dir so the config-bound wrappers land there.
        #[cfg(target_os = "windows")]
        std::env::set_var("USERPROFILE", home.path());
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("HOME", home.path());

        let dest = store_gpx(&input).unwrap();
        let listed = stored_gpx_files().unwrap();

        #[cfg(target_os = "windows")]
        std::env::remove_var("USERPROFILE");
        #[cfg(not(target_os = "windows"))]
        std::env::remove_var("HOME");

        let expected = home
            .path()
            .join(".my-watts")
            .join("gpx")
            .join("wrapped-ride.gpx");
        assert_eq!(dest, expected);
        assert!(listed.contains(&expected));
    }

    #[test]
    fn test_gpx_files_in_lists_only_gpx_sorted() {
        let store = tempfile::tempdir().unwrap();
        write_file(&store.path().join("b.gpx"), "");
        write_file(&store.path().join("a.gpx"), "");
        write_file(&store.path().join("notes.txt"), "");

        let files = gpx_files_in(store.path()).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.gpx", "b.gpx"]);
    }
}
