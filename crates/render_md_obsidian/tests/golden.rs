//! Golden tests: the staged render_md source of the fixture vault must
//! match `tests/golden/` byte for byte. After an intended change, bless the
//! new output with `UPDATE_GOLDEN=1 cargo test -p render_md_obsidian`.

mod common;

use render_md_obsidian::{Diagnostics, stage};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Staged files compared with the golden copies: every page and the nav,
/// but not the theme, which is copied verbatim.
fn staged_files(root: &Path) -> BTreeMap<String, String> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let rel = e
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let keep = rel.ends_with(".md") || rel == "src/@theme/nav.html";
            keep.then(|| (rel, fs::read_to_string(e.path()).unwrap()))
        })
        .collect()
}

#[test]
fn test_staged_site_matches_golden_files() {
    let stage_dir = tempfile::tempdir().unwrap();
    let mut diags = Diagnostics::default();
    stage(
        &common::site_options(common::fixture_vault(), Some("lab/lab.md")),
        stage_dir.path(),
        &mut diags,
    )
    .unwrap();

    let golden_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let actual = staged_files(stage_dir.path());

    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let _ = fs::remove_dir_all(&golden_dir);
        for (rel, content) in &actual {
            let path = golden_dir.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        let report: Vec<String> = diags.iter().map(ToString::to_string).collect();
        fs::write(golden_dir.join("diagnostics.txt"), report.join("\n") + "\n").unwrap();
        return;
    }

    let mut expected = staged_files(&golden_dir);
    let expected_diags = expected
        .remove("diagnostics.txt")
        .or_else(|| fs::read_to_string(golden_dir.join("diagnostics.txt")).ok())
        .unwrap_or_default();
    let actual_names: Vec<&String> = actual.keys().collect();
    let expected_names: Vec<&String> = expected.keys().collect();
    assert_eq!(
        actual_names, expected_names,
        "staged files differ (UPDATE_GOLDEN=1 to bless)"
    );
    for (rel, content) in &actual {
        assert_eq!(
            content, &expected[rel],
            "{rel} differs from tests/golden/{rel} (UPDATE_GOLDEN=1 to bless)"
        );
    }
    let report: Vec<String> = diags.iter().map(ToString::to_string).collect();
    assert_eq!(
        report.join("\n") + "\n",
        expected_diags,
        "diagnostics differ"
    );
}
