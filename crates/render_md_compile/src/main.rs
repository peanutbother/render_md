use clap::Parser;
use render_md::{MAX_INCLUDE_DEPTH_DEFAULT, RenderEngine, RenderOptions, RenderPaths, gray_matter};
use std::{collections::HashMap, path::PathBuf, process::ExitCode};
use walkdir::WalkDir;

/// Ahead-of-time compiler: walks a source directory tree and renders every
/// page to its corresponding static file under `public/`, without needing a
/// running CGI request per page. Intended for build steps / CI, where a site
/// should be fully compiled (and validated) up front rather than lazily on
/// first request, as `render_md_cgi` does.
///
/// Progress is printed to stdout; failures (for individual pages, or for the
/// stylesheet) are printed to stderr and don't abort the run early — every
/// page is attempted, and the process exits non-zero only afterwards, if any
/// failure occurred.
#[derive(Parser, Debug)]
#[command(name = "render_md.compile", version, about)]
struct Args {
    /// Directory containing markdown pages; each route is an `index.md`
    /// (relative to `--base-dir`, unless absolute)
    #[arg(long, default_value = "src")]
    src_dir: PathBuf,

    /// Directory the compiled static output is written to
    /// (relative to `--base-dir`, unless absolute)
    #[arg(long, default_value = "public")]
    public_dir: PathBuf,

    /// Path to the shared HTML template
    /// (relative to `--base-dir`, unless absolute)
    #[arg(long, default_value = "src/template.html")]
    template: PathBuf,

    /// Path to the Tailwind stylesheet entry point
    /// (relative to `--base-dir`, unless absolute)
    #[arg(long, default_value = "src/styles/tailwind.css")]
    style: PathBuf,

    /// Base directory the other paths are resolved against, and that
    /// Tailwind is invoked from
    #[arg(long, default_value = ".")]
    base_dir: PathBuf,

    /// Fallback page title, used unless a page's front matter sets its own
    #[arg(long)]
    title: Option<String>,

    /// Maximum recursion depth for `{{.include ...}}` directives
    #[arg(long, default_value_t = MAX_INCLUDE_DEPTH_DEFAULT)]
    max_include_depth: usize,

    /// Emit miette-formatted, source-mapped errors
    #[cfg(feature = "detailed-errors")]
    #[arg(long)]
    detailed_errors: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    // `compile_styles` invokes Tailwind with its cwd set to `base_dir`, so a
    // relative `base_dir` would otherwise get re-resolved against itself
    // once for the `cd` and again for the (already `base_dir`-joined)
    // `--style` path passed alongside it. Canonicalizing up front keeps
    // every derived path absolute and unambiguous, whether `--base-dir` was
    // given as relative or absolute.
    let base_dir = args.base_dir.canonicalize().unwrap_or_else(|err| {
        eprintln!(
            "warning: failed to canonicalize base dir '{}': {err}",
            args.base_dir.display()
        );
        args.base_dir
    });

    let engine = RenderEngine::<gray_matter::engine::YAML>::new(
        RenderPaths {
            src_dir: base_dir.join(&args.src_dir),
            public_dir: base_dir.join(&args.public_dir),
            template_path: base_dir.join(&args.template),
            style_path: base_dir.join(&args.style),
            base_dir,
        },
        RenderOptions {
            title: args.title,
            #[cfg(feature = "detailed-errors")]
            detailed_errors: args.detailed_errors,
            max_include_depth: args.max_include_depth,
            ..Default::default()
        },
    );

    #[cfg(feature = "detailed-errors")]
    if let Err(err) = miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .wrap_lines(false)
                .color(false)
                .unicode(true)
                .build(),
        )
    })) {
        eprintln!("error: failed to install miette report hook: {err}");
        return ExitCode::FAILURE;
    }

    // Every `index.md` under `src_dir` is a route, including the root page
    // itself (`src_dir/index.md`). `_404.md`/`_500.md` are conventions used
    // by `render_md_cgi`, not routes, and are naturally excluded here since
    // they aren't named `index.md`.
    let mut routes: Vec<PathBuf> = WalkDir::new(&engine.paths.src_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file() && entry.file_name() == "index.md")
        .map(|entry| entry.into_path())
        .collect();
    routes.sort();

    if routes.is_empty() {
        println!("no pages found under '{}'", engine.paths.src_dir.display());
    }

    let mut had_error = false;

    for src_md in routes {
        let route_dir = src_md.parent().unwrap_or(&engine.paths.src_dir);
        let relative = route_dir
            .strip_prefix(&engine.paths.src_dir)
            .unwrap_or(route_dir);
        let public_html = engine.paths.public_dir.join(relative).join("index.html");

        let mut vars = HashMap::new();
        match engine.compile_page(&src_md, &public_html, &mut vars) {
            Ok(()) => println!(
                "compiled '{}' -> '{}'",
                src_md.display(),
                public_html.display()
            ),
            Err(err) => {
                had_error = true;
                eprintln!(
                    "error: failed to compile '{}':\n{}",
                    src_md.display(),
                    engine.format_error(err)
                );
            }
        }
    }

    match engine.compile_styles() {
        Ok(()) => println!(
            "compiled styles -> '{}'",
            engine.paths.public_dir.join("assets/style.css").display()
        ),
        Err(err) => {
            had_error = true;
            eprintln!(
                "error: failed to compile styles:\n{}",
                engine.format_error(err)
            );
        }
    }

    if had_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
