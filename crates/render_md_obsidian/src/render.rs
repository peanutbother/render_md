//! Renders a staged site with render_md's `RenderEngine`, the same loop as
//! `compile_md`, plus what `compile_md` leaves to its caller: assets,
//! vault files used by pages and redirects.

use crate::error::Error;
use crate::site::{Staged, THEME_DIR, write};
use crate::text::escape_html;
use crate::theme;
use render_md::{RenderEngine, RenderOptions, RenderPaths, gray_matter};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Default)]
pub struct RenderReport {
    pub pages: usize,
    /// `(staged page, formatted error)` for pages that failed to render.
    pub failures: Vec<(PathBuf, String)>,
}

pub struct RenderSettings {
    pub skip_styles: bool,
    #[cfg(feature = "detailed-errors")]
    pub detailed_errors: bool,
}

pub fn render(
    staged: &Staged,
    out: &Path,
    settings: &RenderSettings,
) -> Result<RenderReport, Error> {
    let src = staged.dir.join("src");
    let theme_dir = src.join(THEME_DIR);
    let engine = RenderEngine::<gray_matter::engine::YAML>::new(
        RenderPaths {
            base_dir: staged.dir.clone(),
            src_dir: src.clone(),
            public_dir: out.to_owned(),
            template_path: theme_dir.join("template.html"),
            style_path: theme_dir.join("styles/tailwind.css"),
        },
        RenderOptions {
            title: Some(staged.title.clone()),
            #[cfg(feature = "detailed-errors")]
            detailed_errors: settings.detailed_errors,
            ..Default::default()
        },
    );

    let mut pages: Vec<PathBuf> = WalkDir::new(&src)
        .into_iter()
        .filter_entry(|e| e.file_name() != THEME_DIR)
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.file_name() == "index.md")
        .map(|e| e.into_path())
        .collect();
    pages.sort();

    let mut report = RenderReport::default();
    for page in pages {
        let route_dir = page.parent().unwrap_or(&src);
        let relative = route_dir.strip_prefix(&src).unwrap_or(route_dir);
        let public_html = out.join(relative).join("index.html");
        match engine.compile_page(&page, &public_html, &mut HashMap::new()) {
            Ok(()) => report.pages += 1,
            Err(err) => report.failures.push((page, engine.format_error(err))),
        }
    }

    for (route, target) in &staged.redirects {
        let target = escape_html(target);
        let html = format!(
            "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0; url={target}\"><link rel=\"canonical\" href=\"{target}\"></head><body><a href=\"{target}\">{target}</a></body></html>\n"
        );
        write(&out.join(route.trim_matches('/')).join("index.html"), &html)?;
    }
    for (source, url) in &staged.files {
        theme::copy(source, &out.join(url.trim_start_matches('/')))?;
    }
    theme::write_static(&out.join("assets"), staged.static_override.as_deref())?;
    if !settings.skip_styles {
        engine.compile_styles()?;
    }
    Ok(report)
}
