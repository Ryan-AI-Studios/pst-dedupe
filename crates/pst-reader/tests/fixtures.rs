//! Fixture discovery helpers for pst-reader integration tests.
//!
//! Tests that require real PST files should call [`discover_pst_fixtures`]
//! and skip gracefully when none are available.

use std::path::PathBuf;

/// Directory where PST fixtures are expected, relative to the workspace root.
const FIXTURE_DIR: &str = "fixtures";

/// Discover all `.pst` files in the fixture directory.
///
/// Returns an empty vec if the directory does not exist or contains no PSTs.
/// This allows tests to skip rather than fail on machines without fixtures.
pub fn discover_pst_fixtures() -> Vec<PathBuf> {
    let workspace_root = workspace_root();
    let fixture_path = workspace_root.join(FIXTURE_DIR);

    if !fixture_path.is_dir() {
        return Vec::new();
    }

    std::fs::read_dir(&fixture_path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("pst"))
                .unwrap_or(false)
        })
        .map(|entry| entry.path())
        .collect()
}

/// Return the workspace root directory.
///
/// Assumes tests run from within `target/debug/deps/` under the crate directory.
fn workspace_root() -> PathBuf {
    // Cargo test binaries run from target/debug/deps/ inside the crate.
    // Walk up: deps -> debug -> target -> crate -> workspace
    let mut path = std::env::current_exe()
        .expect("current_exe available")
        .canonicalize()
        .expect("canonicalize current_exe");

    // target/debug/deps -> target/debug -> target -> crate
    for _ in 0..3 {
        if !path.pop() {
            panic!("Could not find workspace root from test binary path");
        }
    }

    // crate -> workspace
    if !path.pop() {
        panic!("Could not find workspace root from crate directory");
    }

    path
}

/// Return the first discovered fixture, or `None`.
pub fn first_fixture() -> Option<PathBuf> {
    discover_pst_fixtures().into_iter().next()
}

/// Assert that at least one fixture exists, returning the first one.
/// Panics with a helpful message if none are found.
#[allow(dead_code)]
pub fn require_fixture() -> PathBuf {
    first_fixture().unwrap_or_else(|| {
        let root = workspace_root();
        panic!(
            "No PST fixtures found in {}. \
             Place .pst files in {} to run integration tests.",
            root.join(FIXTURE_DIR).display(),
            root.join(FIXTURE_DIR).display()
        )
    })
}

/// Check whether a fixture with the given name (without extension) exists.
#[allow(dead_code)]
pub fn fixture_named(name: &str) -> Option<PathBuf> {
    let root = workspace_root();
    let path = root.join(FIXTURE_DIR).join(format!("{name}.pst"));
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}
