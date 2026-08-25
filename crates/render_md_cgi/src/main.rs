use std::{env, path::PathBuf};

/// This application is used as cgi-bin and reads environment variables to
/// determine options for rendering. See [`render_md_cgi::engine_from_env`]
/// for the environment variables it reads.
fn main() {
    let uri = env::var("DOCUMENT_URI").unwrap_or_else(|_| "/".to_string());
    let engine = render_md_cgi::engine_from_env(PathBuf::from("."));

    render_md_cgi::serve(uri, engine);
}
