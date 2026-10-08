//! The built-in theme (Tailwind v4 + daisyUI, from the `cgi_bin` example),
//! compiled into the binary so `compile_vault` is a single file.
//!
//! Layout of the theme, and of a `--theme` directory overriding it:
//!
//! ```text
//! template.html        page shell ({{title}}, {{body}}, {{.include nav.html}})
//! styles/tailwind.css  stylesheet entry point (and anything it imports)
//! static/              copied to /assets/
//! ```

use crate::error::Error;
use crate::site::write;
use std::fs;
use std::path::Path;

const TEMPLATE: &str = include_str!("../theme/template.html");
const STYLES: &[(&str, &str)] = &[
    ("tailwind.css", include_str!("../theme/styles/tailwind.css")),
    ("daisyui.mjs", include_str!("../theme/styles/daisyui.mjs")),
    (
        "daisyui-theme.mjs",
        include_str!("../theme/styles/daisyui-theme.mjs"),
    ),
];
const STATIC: &[(&str, &str)] = &[
    ("logo.svg", include_str!("../theme/static/logo.svg")),
    ("code.css", include_str!("../theme/static/code.css")),
];

/// Writes the theme into the staged `src/@theme/`, then copies
/// `override_dir`'s `template.html` and `styles/` over it.
pub fn write_theme(dir: &Path, override_dir: Option<&Path>) -> Result<(), Error> {
    write(&dir.join("template.html"), TEMPLATE)?;
    for (name, content) in STYLES {
        write(&dir.join("styles").join(name), content)?;
    }
    if let Some(theme) = override_dir {
        if !theme.is_dir() {
            return Err(Error::theme_not_found(theme));
        }
        let template = theme.join("template.html");
        if template.is_file() {
            copy(&template, &dir.join("template.html"))?;
        }
        copy_dir(&theme.join("styles"), &dir.join("styles"))?;
    }
    Ok(())
}

/// Writes the theme's static files into `assets`, then the override's.
pub fn write_static(assets: &Path, override_static: Option<&Path>) -> Result<(), Error> {
    for (name, content) in STATIC {
        write(&assets.join(name), content)?;
    }
    if let Some(dir) = override_static {
        copy_dir(dir, assets)?;
    }
    Ok(())
}

pub(crate) fn copy(from: &Path, to: &Path) -> Result<(), Error> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::write(e, parent))?;
    }
    fs::copy(from, to).map_err(|e| Error::read(e, from))?;
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), Error> {
    if !from.is_dir() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(from) {
        let entry = entry.map_err(|e| Error::read(std::io::Error::from(e), from))?;
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(from).unwrap_or(entry.path());
            copy(entry.path(), &to.join(rel))?;
        }
    }
    Ok(())
}
