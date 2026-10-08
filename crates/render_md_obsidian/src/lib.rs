//! Turns an Obsidian vault into a static site rendered with render_md.
//!
//! The vault is transpiled into a staging render_md site (one
//! `src/<route>/index.md` per note, plus the theme and a sidebar partial),
//! which [`RenderEngine`](render_md::RenderEngine) then renders. All the
//! Obsidian handling is source to source:
//!
//! 1. front matter is split off and kept for the properties table and bases
//! 2. `%% comments %%` are removed ([`comments`])
//! 3. one offset pass ([`scan`]) finds wikilinks, embeds, headings, block
//!    ids, tags and query blocks; links are resolved ([`resolve`]) and
//!    rewritten, headings get ids ([`transform`])
//! 4. callouts become `<details>`/`<div>` blocks ([`callouts`])
//! 5. embeds are expanded: notes, sections, `.base` tables ([`bases`]),
//!    images
//! 6. `{{.` is escaped so render_md doesn't read it as a directive
//! 7. the page is written with breadcrumbs, properties and backlinks
//!    ([`site`])

pub mod bases;
pub mod callouts;
pub mod comments;
pub mod diagnostics;
pub mod error;
pub mod glob;
pub mod render;
pub mod resolve;
pub mod scan;
pub mod site;
pub mod slug;
pub mod text;
pub mod theme;
pub mod transform;
pub mod vault;

pub use diagnostics::{Diagnostic, Diagnostics, Severity};
pub use error::{Error, ErrorKind};
pub use render::{RenderReport, RenderSettings, render};
pub use site::{SiteOptions, Staged, stage};
pub use transform::EmbedMode;

use std::path::{Path, PathBuf};

/// Everything [`build`] needs.
pub struct BuildOptions {
    pub site: SiteOptions,
    pub out: PathBuf,
    /// Keep the staging tree here; a temporary directory otherwise.
    pub stage: Option<PathBuf>,
    pub render: RenderSettings,
}

/// The outcome of a [`build`].
#[derive(Debug)]
pub struct BuildReport {
    pub staged: Staged,
    pub render: RenderReport,
    pub diagnostics: Diagnostics,
}

/// Stages the vault and renders it into `options.out`.
pub fn build(mut options: BuildOptions) -> Result<BuildReport, Error> {
    // Tailwind runs with the staging directory as its working directory, so
    // a relative `--out` would put the stylesheet into the stage.
    let absolute = |path: &Path| std::path::absolute(path).map_err(|e| Error::read(e, path));
    options.out = absolute(&options.out)?;
    options.site.vault = absolute(&options.site.vault)?;
    if let Some(theme) = &options.site.theme {
        options.site.theme = Some(absolute(theme)?);
    }
    if let Some(stage) = &options.stage {
        options.stage = Some(absolute(stage)?);
    }
    options.site.skip_dirs.push(options.out.clone());
    let temp;
    let stage_dir: &Path = match &options.stage {
        Some(dir) => {
            options.site.skip_dirs.push(dir.clone());
            // Pages of deleted notes must not survive in a kept stage, but
            // only a previous stage is ever cleared.
            let src = dir.join("src");
            if src.exists() {
                if !src.join(site::THEME_DIR).is_dir() {
                    return Err(Error::stage_not_empty(dir));
                }
                std::fs::remove_dir_all(&src).map_err(|e| Error::write(e, &src))?;
            }
            dir
        }
        None => {
            temp = tempfile::tempdir().map_err(|e| Error::write(e, std::env::temp_dir()))?;
            temp.path()
        }
    };

    let mut diagnostics = Diagnostics::default();
    let staged = stage(&options.site, stage_dir, &mut diagnostics)?;
    let render = render(&staged, &options.out, &options.render)?;
    Ok(BuildReport {
        staged,
        render,
        diagnostics,
    })
}
