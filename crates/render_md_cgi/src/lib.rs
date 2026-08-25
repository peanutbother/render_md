use http::StatusCode;
use render_md::{MAX_INCLUDE_DEPTH_DEFAULT, RenderEngine, RenderOptions, RenderPaths, gray_matter};
use std::{
    collections::HashMap,
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Builds a `RenderEngine` from the standard cgi-bin environment variables,
/// rooted at `default_app_root` unless `APP_ROOT` overrides it. Shared by
/// this crate's own `render_md_cgi` binary and by its `cgi_bin` example, so
/// both configure themselves identically and only differ in what root they
/// default to when `APP_ROOT` isn't set.
///
/// Used environment:
/// - `APP_ROOT`: base directory containing `src/` (pages, `template.html`,
///   `styles/`) and `public/`; defaults to `default_app_root`
/// - `DOCUMENT_TITLE`: used for title and as templating variable
/// - `MAX_INCLUDE_DEPTH`: used to overwrite maximum inclusion depth to set recursion limit
/// - `DETAILED_ERRORS`: used to display detailed errors in templating and of this application. Parses values to true if "1", "yes", "true" or "enable"
pub fn engine_from_env(
    default_app_root: impl Into<PathBuf>,
) -> RenderEngine<gray_matter::engine::YAML> {
    #[cfg(feature = "detailed-errors")]
    let detailed_errors = env::var("DETAILED_ERRORS")
        .map(|v| v.to_lowercase())
        .map(|v| v == "1" || v == "yes" || v == "true" || v == "enable")
        .unwrap_or_default();
    let max_include_depth = env::var("MAX_INCLUDE_DEPTH")
        .map(|v| v.parse().unwrap_or(MAX_INCLUDE_DEPTH_DEFAULT))
        .unwrap_or(MAX_INCLUDE_DEPTH_DEFAULT);
    let base_path = env::var("APP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_app_root.into());
    let title = env::var("DOCUMENT_TITLE").ok();

    RenderEngine::new(
        RenderPaths {
            src_dir: base_path.join("src"),
            public_dir: base_path.join("public"),
            template_path: base_path.join("src/template.html"),
            style_path: base_path.join("src/styles/tailwind.css"),
            base_dir: base_path,
        },
        RenderOptions {
            title,
            #[cfg(feature = "detailed-errors")]
            detailed_errors,
            max_include_depth,
            ..Default::default()
        },
    )
}

/// Handles rendering of the site with given configured rendering engine.
///
/// If rendering fails [handle_error] is called to gracefully render errors or falls back to print the error verbatim.
pub fn serve(uri: String, engine: RenderEngine<gray_matter::engine::YAML>) {
    let mut vars = HashMap::new();
    #[cfg(feature = "detailed-errors")]
    if let Err(err) = miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .wrap_lines(false) // Disable line wrapping so Error messages are consistent
                .color(false) // Turn off ANSI terminal color codes
                .unicode(true) // Keep the nice ASCII/Unicode tree drawings
                .build(),
        )
    })) {
        handle_error(&engine, &mut vars, render_md::Error::miette(err));
        return;
    }

    let (route_dir, src_md, public_html) = match engine.resolve_paths(&uri) {
        Some(paths) => paths,
        None => match engine.render_page(&engine.paths.src_dir.join("_404.md"), &mut vars) {
            Ok(page) => {
                engine.render_error(StatusCode::NOT_FOUND, &page);
                return;
            }
            Err(err) => {
                handle_error(&engine, &mut vars, err);
                return;
            }
        },
    };
    if engine.is_stale(&route_dir, &public_html) {
        if let Err(e) = engine.compile_page(&src_md, &public_html, &mut vars) {
            handle_error(&engine, &mut vars, e);
            return;
        }
        if let Err(e) = engine.compile_styles() {
            handle_error(&engine, &mut vars, e);
            return;
        }
    }
    serve_file(&public_html);
}

/// Handles rendering errors by displaying a formatted error page (typically `_500.md`).
///
/// If rendering the error page itself fails, a plain text fallback error message is shown.
pub fn handle_error<T: gray_matter::engine::Engine>(
    engine: &RenderEngine<T>,
    vars: &mut HashMap<String, String>,
    err: render_md::Error,
) {
    let err_msg = engine.format_error(err);
    vars.insert("error".to_owned(), err_msg);

    match engine.render_page(&engine.paths.src_dir.join("_500.md"), vars) {
        Ok(page) => engine.render_error(StatusCode::INTERNAL_SERVER_ERROR, &page),
        Err(fallback_err) => {
            engine.render_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!(
                    "Critical: Failed to render 500 page: {}\nOriginal error was: {}",
                    fallback_err,
                    vars.get("error").unwrap_or(&"unknown".to_string())
                ),
            );
        }
    }
}

/// Serves a static file to standard output, prefixed with HTTP headers.
///
/// This is used for CGI responses to stream the compiled HTML page to the web server.
pub fn serve_file(path: &Path) {
    println!(
        "Status: {}\r\nContent-Type: text/html; charset=utf-8\r\n\r\n",
        StatusCode::OK
    );
    if let Ok(mut file) = fs::File::open(path) {
        let mut buffer = Vec::new();
        if file.read_to_end(&mut buffer).is_ok() {
            let _ = std::io::stdout().write_all(&buffer);
        }
    }
}
