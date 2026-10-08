//! Shared helpers for the integration tests.

#![allow(dead_code)]

use render_md_obsidian::{BuildOptions, EmbedMode, RenderSettings, SiteOptions};
use std::path::{Path, PathBuf};

pub fn fixture_vault() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
}

pub fn site_options(vault: PathBuf, home: Option<&str>) -> SiteOptions {
    SiteOptions {
        vault,
        home: home.map(str::to_owned),
        excludes: Vec::new(),
        hidden_properties: vec!["proxy_*".to_owned()],
        embeds: EmbedMode::Collapsed,
        max_embed_depth: 10,
        title: Some("Lab".to_owned()),
        theme: None,
        skip_dirs: Vec::new(),
    }
}

pub fn build_options(site: SiteOptions, out: &Path, stage: Option<&Path>) -> BuildOptions {
    BuildOptions {
        site,
        out: out.to_owned(),
        stage: stage.map(Path::to_owned),
        render: RenderSettings {
            skip_styles: true,
            #[cfg(feature = "detailed-errors")]
            detailed_errors: false,
        },
    }
}

/// The HTML outside `<pre>`, `<code>` and `<script>`, where no Obsidian
/// syntax may survive.
pub fn prose(html: &str) -> String {
    let mut out = html.to_owned();
    for tag in ["pre", "code", "script"] {
        while let Some(start) = out.find(&format!("<{tag}")) {
            let close = format!("</{tag}>");
            let Some(end) = out[start..].find(&close) else {
                break;
            };
            out.replace_range(start..start + end + close.len(), "");
        }
    }
    out
}
