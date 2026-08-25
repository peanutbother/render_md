use std::env;

/// A working example of `render_md_cgi` driving a small, Tailwind-styled
/// personal site through classic CGI. See `README.md` in this directory for
/// the full write-up (layout, request flow, nginx wiring).
///
/// Run it from the repo root with `cargo run --example cgi_bin -p
/// render_md_cgi` — `APP_ROOT` defaults to this example's own directory
/// (via `CARGO_MANIFEST_DIR`), not the process's current directory, so it
/// doesn't matter where `cargo run` is invoked from.
///
/// See [`render_md_cgi::engine_from_env`] for the environment variables read.
fn main() {
    let uri = env::var("DOCUMENT_URI").unwrap_or_else(|_| "/".to_string());
    let app_root = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/cgi_bin");
    let engine = render_md_cgi::engine_from_env(app_root);

    render_md_cgi::serve(uri, engine);
}
