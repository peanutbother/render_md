use std::path::PathBuf;

/// Errors that stop a vault build. Problems inside notes (broken links,
/// unsupported bases, ...) are collected as
/// [`Diagnostics`](crate::diagnostics::Diagnostics) instead.
#[derive(
    Debug,
    thiserror::Error,
    miette::Diagnostic,
    thiserror_ext::Box,
    thiserror_ext::Construct,
    thiserror_ext::Macro,
)]
#[thiserror_ext(newtype(name = Error))]
pub enum ErrorKind {
    /// A file or directory could not be read.
    #[error("Failed to read '{path}'")]
    #[diagnostic(code(render_md_obsidian::read))]
    Read {
        /// The path that failed.
        path: PathBuf,
        /// The underlying IO error.
        source: std::io::Error,
    },

    /// A staged or output file could not be written.
    #[error("Failed to write '{path}'")]
    #[diagnostic(code(render_md_obsidian::write))]
    Write {
        /// The path that failed.
        path: PathBuf,
        /// The underlying IO error.
        source: std::io::Error,
    },

    /// The `--vault` path is not a directory.
    #[error("Vault '{path}' is not a directory")]
    #[diagnostic(
        code(render_md_obsidian::not_a_directory),
        help("Pass the vault's root folder, the one containing '.obsidian/'.")
    )]
    NotADirectory {
        /// The path given as vault.
        path: PathBuf,
    },

    /// The `--home` note doesn't exist.
    #[error("Home note '{home}' is not a note in the vault")]
    #[diagnostic(
        code(render_md_obsidian::home_not_found),
        help("Give the note's vault-relative path, e.g. 'notes/home.md'.")
    )]
    HomeNotFound {
        /// The path given as home note.
        home: String,
    },

    /// Two pages map to the same URL.
    #[error("'{first}' and '{second}' both map to the URL '{route}'")]
    #[diagnostic(
        code(render_md_obsidian::route_collision),
        help("Rename one of them, or leave one out with '--exclude'.")
    )]
    RouteCollision {
        /// The URL path both claim.
        route: String,
        /// The first owner (a vault path).
        first: String,
        /// The second owner (a vault path).
        second: String,
    },

    /// The `--theme` directory doesn't exist.
    #[error("Theme directory '{path}' does not exist")]
    #[diagnostic(code(render_md_obsidian::theme_not_found))]
    ThemeNotFound {
        /// The path given as theme.
        path: PathBuf,
    },

    /// The `--stage` directory has a `src/` that isn't a previous stage.
    #[error("'{path}' already contains a 'src' folder that wasn't staged by compile_vault")]
    #[diagnostic(
        code(render_md_obsidian::stage_not_empty),
        help("Pass an empty or new directory, or one used as '--stage' before.")
    )]
    StageNotEmpty {
        /// The staging directory.
        path: PathBuf,
    },

    /// An error from the render_md engine (rendering pages, Tailwind).
    #[error(transparent)]
    #[diagnostic(code(render_md_obsidian::render))]
    Render(#[from] render_md::Error),
}
