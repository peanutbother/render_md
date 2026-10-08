use clap::{Parser, ValueEnum};
use render_md_obsidian::{BuildOptions, EmbedMode, RenderSettings, SiteOptions, build};
use std::{path::PathBuf, process::ExitCode};

/// Renders an Obsidian vault into a static site: wikilinks, embeds,
/// callouts, properties, bases and kanban boards become plain HTML pages,
/// rendered with render_md.
///
/// Problems inside notes (broken links, unsupported bases, ...) are printed
/// to stderr; with `--strict`, broken links and embeds and pages that fail
/// to render make the run fail.
#[derive(Parser, Debug)]
#[command(name = "compile_vault", version, about)]
struct Args {
    /// The vault's root folder
    #[arg(long)]
    vault: PathBuf,

    /// Directory the site is written to (existing files are overwritten,
    /// never deleted)
    #[arg(long)]
    out: PathBuf,

    /// Keep the generated render_md source tree here, for debugging
    /// (default: a temporary directory)
    #[arg(long)]
    stage: Option<PathBuf>,

    /// Directory overriding the built-in theme: `template.html`,
    /// `styles/tailwind.css`, `static/`
    #[arg(long)]
    theme: Option<PathBuf>,

    /// Vault-relative path of the note served at `/` (default: a note at the
    /// vault root named like the site title, or index, home or readme)
    #[arg(long)]
    home: Option<String>,

    /// Leave out files or folders matching this glob (repeatable);
    /// dot-files and `*.blueprint` are always left out
    #[arg(long = "exclude", value_name = "GLOB")]
    excludes: Vec<String>,

    /// Hide properties matching this glob from the properties table
    /// (repeatable), e.g. 'internal_*'
    #[arg(long = "hide-property", value_name = "GLOB")]
    hidden_properties: Vec<String>,

    /// How to show embedded notes (`![[note]]`)
    #[arg(long, value_enum, default_value_t = Embeds::Collapsed)]
    embeds: Embeds,

    /// Maximum nesting of embeds inside embeds
    #[arg(long, default_value_t = 10)]
    max_embed_depth: usize,

    /// Site title, shown in the sidebar and browser tabs (default: the
    /// vault folder's name)
    #[arg(long)]
    title: Option<String>,

    /// Title of the page at `/` (default: the home note's name)
    #[arg(long)]
    home_title: Option<String>,

    /// Fail on broken links, broken embeds and pages that fail to render
    #[arg(long)]
    strict: bool,

    /// Don't compile the stylesheet (no `tailwindcss` needed)
    #[arg(long)]
    skip_styles: bool,

    /// Emit miette-formatted, source-mapped render errors
    #[cfg(feature = "detailed-errors")]
    #[arg(long)]
    detailed_errors: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Embeds {
    /// In a closed `<details>` block
    Collapsed,
    /// Inline, in a bordered block
    Inline,
}

fn main() -> ExitCode {
    let args = Args::parse();

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

    let options = BuildOptions {
        site: SiteOptions {
            vault: args.vault,
            home: args.home,
            excludes: args.excludes,
            hidden_properties: args.hidden_properties,
            embeds: match args.embeds {
                Embeds::Collapsed => EmbedMode::Collapsed,
                Embeds::Inline => EmbedMode::Inline,
            },
            max_embed_depth: args.max_embed_depth,
            title: args.title,
            home_title: args.home_title,
            theme: args.theme,
            skip_dirs: Vec::new(),
        },
        out: args.out.clone(),
        stage: args.stage,
        render: RenderSettings {
            skip_styles: args.skip_styles,
            #[cfg(feature = "detailed-errors")]
            detailed_errors: args.detailed_errors,
        },
    };

    let report = match build(options) {
        Ok(report) => report,
        Err(err) => {
            report_error(err);
            return ExitCode::FAILURE;
        }
    };

    for diagnostic in report.diagnostics.iter() {
        eprintln!("{diagnostic}");
    }
    for (page, error) in &report.render.failures {
        eprintln!("error: failed to render '{}':\n{error}", page.display());
    }
    let broken = report.diagnostics.broken_count();
    println!(
        "{} notes, {} generated pages: rendered {} pages into '{}' ({} broken, {} warnings, {} render errors)",
        report.staged.notes,
        report.staged.generated_pages,
        report.render.pages,
        args.out.display(),
        broken,
        report.diagnostics.len() - broken,
        report.render.failures.len(),
    );

    if !report.render.failures.is_empty() || (args.strict && broken > 0) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Prints a fatal error: a miette report when built with
/// `detailed-errors`, plain `error:`/`help:` lines otherwise.
fn report_error(err: render_md_obsidian::Error) {
    #[cfg(feature = "detailed-errors")]
    eprintln!("{:?}", miette::Report::new(err.into_inner()));
    #[cfg(not(feature = "detailed-errors"))]
    {
        use miette::Diagnostic;
        use std::error::Error as _;
        let kind = err.inner();
        eprintln!("error: {kind}");
        let mut source = kind.source();
        while let Some(cause) = source {
            eprintln!("  caused by: {cause}");
            source = cause.source();
        }
        if let Some(help) = kind.help() {
            eprintln!("help: {help}");
        }
    }
}
