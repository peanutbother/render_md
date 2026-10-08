//! Smoke test against a real vault, never run in CI:
//!
//! ```sh
//! VAULT_DIR=/path/to/vault VAULT_HOME=home/home.md \
//!     cargo test -p render_md_obsidian -- --ignored --nocapture
//! ```

mod common;

use render_md_obsidian::build;
use std::fs;
use walkdir::WalkDir;

#[test]
#[ignore = "needs VAULT_DIR"]
fn test_real_vault_renders_without_errors() {
    let Some(vault) = std::env::var_os("VAULT_DIR") else {
        panic!("set VAULT_DIR to the vault to check");
    };
    let home = std::env::var("VAULT_HOME").ok();
    let out = tempfile::tempdir().unwrap();
    let mut site = common::site_options(vault.into(), home.as_deref());
    site.title = None;
    let report = build(common::build_options(site, out.path(), None)).unwrap();

    for diagnostic in report.diagnostics.iter() {
        println!("{diagnostic}");
    }
    assert!(
        report.render.failures.is_empty(),
        "{:?}",
        report.render.failures
    );

    let mut leftovers = Vec::new();
    for entry in WalkDir::new(out.path()).into_iter().filter_map(Result::ok) {
        if entry.file_name() != "index.html" {
            continue;
        }
        let prose = common::prose(&fs::read_to_string(entry.path()).unwrap());
        for marker in ["[[", "[!", "%%", "\u{E000}"] {
            if prose.contains(marker) {
                leftovers.push(format!("{marker} in {}", entry.path().display()));
            }
        }
    }
    assert!(leftovers.is_empty(), "{leftovers:#?}");
    println!(
        "{} notes, {} pages, {} broken",
        report.staged.notes,
        report.render.pages,
        report.diagnostics.broken_count()
    );
}
