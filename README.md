# <img src="crates/render_md_cgi/examples/cgi_bin/static/logo.svg" alt="render_md" width="30"></img> render_md

A small Rust library for turning a directory of Markdown files into a static site (or a lazily-rendered, cached one), with a minimal templating language for variables, includes, conditionals, and styled blocks layered on
top of [pulldown-cmark](https://crates.io/crates/pulldown-cmark).

It was built to power CGI-rendered personal/homelab sites,=500x300=500x300 but the core
`RenderEngine` has no web-server or CGI dependency — it just reads files and
returns/writes HTML strings, so it's equally usable from a one-off static
site generator, a build script, or a long-running server.

## Crates

This is a workspace with three members:

| Crate | Path | What it is |
| --- | --- | --- |
| [`render_md`](#render_md-library) | [`crates/render_md`](crates/render_md) | The library: `RenderEngine`, the template directive language, Markdown rendering, Tailwind integration. |
| `render_md_cgi` | [`crates/render_md_cgi`](crates/render_md_cgi) | A `render_md`-based CGI binary (`render_md`) plus a small reusable library (`engine_from_env`/`serve`/`handle_error`) for classic CGI deployments, with a full worked example. |
| `render_md_compile` | [`crates/render_md_compile`](crates/render_md_compile) | `compile_md`, a CLI that statically compiles a source directory ahead of time, for build steps/CI instead of lazy per-request rendering. |

```text
.
├─ Cargo.toml                     # workspace manifest
├─ crates/
│  ├─ render_md/                  # the library
│  ├─ render_md_cgi/
│  │  ├─ src/                     # `render_md` bin + `render_md_cgi` lib
│  │  └─ examples/cgi_bin/        # full worked example (see its own README)
│  └─ render_md_compile/          # `compile_md` bin
└─ flake.nix                      # Nix dev shell / build
```

## render_md (library)

### Features

- **Markdown → HTML** via `pulldown-cmark`, with tables, footnotes,
  strikethrough, task lists, heading attributes, and wikilinks enabled.
- **Front matter** per page (via [`gray_matter`](https://crates.io/crates/gray_matter), so YAML, TOML, JSON, or any
  format implementing its `Engine` trait) for title, custom variables, an
  `args` schema, and per-page tag/block class overrides.
- **A tiny template directive language** (`{{.var}}`, `{{.include}}`,
  `{{.if}}/{{.else}}/{{.end}}`, `{{.block}}/{{.end}}`) usable in both
  Markdown pages and the surrounding HTML template/partials.
- **Reusable components**: any included `.md`/`.html` file can declare its
  own required/default arguments in front matter, turning it into a
  parameterized partial.
- **CSS class injection**: map standard Markdown-generated tags (`h1`–`h6`,
  `p`, `blockquote`, `code`, `ul`/`ol`, `li`) to your own utility classes
  (e.g. Tailwind) without writing a custom renderer.
- **Lazy, staleness-aware compilation**: `is_stale`/`compile_page` compare
  modification times of the template, stylesheet, and source tree so a
  page is only (re)compiled on request, and only when something it
  depends on has actually changed since the cached output was written.
- **Tailwind CSS integration**: shells out to a `tailwindcss` binary to
  compile a stylesheet into the public output directory.
- **Rich, spanned errors** via [`miette`](https://crates.io/crates/miette), optionally rendered with
  source snippets and hints behind the `detailed-errors` feature.

### Installation

Not yet published to crates.io — depend on it via git or a local path:

```toml
[dependencies]
render_md = { git = "https://git.yuna.pw/yuna/render_md" }
# or, from elsewhere in this workspace:
render_md = { path = "../render_md" }
```

Enable detailed, source-mapped error output (uses `miette`'s `fancy`
feature) with:

```toml
render_md = { version = "...", features = ["detailed-errors"] }
```

`render_md` re-exports `gray_matter` as `render_md::gray_matter`, so you
don't need to add it as a separate dependency just to name a front-matter
engine — see [Quick start](#quick-start).

### Project layout the engine expects

`RenderEngine` doesn't scaffold a project for you — you point it at paths
you already have on disk via `RenderPaths`. Nothing about the library
requires this exact shape, but it's the convention both `render_md_cgi` and
`render_md_compile` default to:

```text
<base_dir>/
├─ src/                # src_dir: markdown pages + partials, mirrors the URL tree
│  ├─ index.md
│  ├─ about/
│  │  └─ index.md      # -> route "/about"
│  ├─ navbar.html      # a partial, included from elsewhere
│  ├─ template.html    # template_path: the shared HTML shell
│  └─ styles/
│     └─ tailwind.css  # style_path: input stylesheet
└─ public/              # public_dir: compiled HTML + assets/style.css go here
```

A route's markdown file must be named `index.md` inside a directory that
mirrors the URL path — `resolve_paths("/about")` looks for
`src/about/index.md` and writes to `public/about/index.html`. `src_dir`,
`template_path`, and `style_path` are independent fields on `RenderPaths`,
so nothing stops `template.html`/`styles/` from living outside `src_dir` if
you'd rather keep pages separate — this is just what this repo's own
binaries use.

### Quick start

```rust
use render_md::{RenderEngine, RenderOptions, RenderPaths};
use render_md::gray_matter::engine::YAML;
use std::collections::HashMap;

let engine = RenderEngine::<YAML>::new(
    RenderPaths {
        base_dir: "site".into(),
        src_dir: "site/src".into(),
        public_dir: "site/public".into(),
        template_path: "site/src/template.html".into(),
        style_path: "site/src/styles/tailwind.css".into(),
    },
    RenderOptions {
        title: Some("My Site".into()),
        ..Default::default()
    },
);

let (route_dir, src_md, public_html) = engine
    .resolve_paths("/about")
    .expect("route not found");

let mut vars = HashMap::new();
if engine.is_stale(&route_dir, &public_html) {
    engine.compile_page(&src_md, &public_html, &mut vars)?;
    engine.compile_styles()?;
}
```

`RenderEngine` is generic over `T: gray_matter::engine::Engine`, which
selects the front-matter dialect — `YAML` (shown above, from the re-exported
`render_md::gray_matter::engine`) is the usual choice; JSON and TOML engines
also exist upstream.

`render_page` returns the rendered HTML as a `String` (useful for CGI/server
handlers that stream a response directly); `compile_page` does the same and
also writes the result to `public_html`, creating parent directories as
needed.

### Front matter

Each page (and each included component) may start with a front-matter
block, in whichever format the engine's `T` is set up to parse — YAML shown
here:

```markdown
---
title: About
tags:
  h1: text-4xl font-bold
  p: leading-6
blocks:
  hero: bg-hero p-8
args:
  greeter: "stranger"   # optional: has a default
  visitor:              # required: no default, errors if unused... unless referenced
custom_var: some value
---

# Hello {{.var custom_var}}
```

- `title` overrides `RenderOptions::title` for this page.
- `tags`/`blocks` are merged onto (and override) the engine-wide defaults
  from `RenderOptions` for the duration of this page's render.
- `args` declares this page/component's parameter schema when it's used as
  an `.include` target — see [Components & arguments](#components--arguments).
- Any other top-level key is exposed as a `{{.var ...}}`-accessible
  variable in the page's own scope.

`RenderOptions` (title, `tags`, `blocks`, `max_include_depth`, and — behind
`detailed-errors` — `detailed_errors`) is constructed programmatically and is
never itself deserialized from a page's front matter, so a single page can't
silently override process-wide settings.

### Template directives

Directives are written as `{{.name ...}}` and work in both Markdown pages
and HTML partials/templates. They're evaluated before the Markdown parser
runs, so their output becomes part of the Markdown source.

| Directive | Example | Behavior |
| --- | --- | --- |
| `.var` | `{{.var name}}` | Interpolates a variable. Errors (`VariableNotFound`) if it isn't set. |
| `.var` with filters | `{{.var name \| upper \| trim}}` | Chains filters (`upper`, `lower`, `trim`, `title`) over the resolved value. |
| `.include` | `{{.include partial.md}}` | Inlines another file's rendered content, resolved relative to the including file. |
| `.include` with args | `{{.include card.md title="Hi {{.var name}}"}}` | Passes arguments to a component (see below). Quoted values may themselves contain nested directives. |
| `.if` / `.else` / `.end` | `{{.if flag}}Yes{{.else}}No{{.end}}` | Conditional rendering (see below). |
| `.block` / `.end` | `{{.block hero}}...{{.end}}` | Wraps content in `<div class="...">`, resolving `hero` against the engine's `blocks` map (falls back to using `hero` itself as the class list if unmapped). |

`.if` conditions support:

- `{{.if flag}}` — truthy if `flag` is set and non-empty.
- `{{.if !flag}}` — negation.
- `{{.if a == "literal"}}` / `{{.if a != b}}` — string comparison between a
  variable and either a quoted literal or another variable. A missing
  variable resolves to falsy/empty rather than erroring, unlike `.var`.

`.include`d HTML files (as opposed to `.md`) get their content trimmed and
padded with blank lines so pulldown-cmark treats them as a standalone HTML
block rather than folding them into surrounding paragraph text.

### Components & arguments

Any included file can declare an `args` schema in its own front matter.
Each key with a value is an optional argument with that default; each key
with no value (`name:`) is required, and omitting it raises
`MissingIncludeArgument`.

```markdown
<!-- greeting.md -->
---
args:
  name: "stranger"
---
Hi {{.var name}}
```

```markdown
{{.include greeting.md}}                 <!-- "Hi stranger" -->
{{.include greeting.md name="Bob"}}      <!-- "Hi Bob" -->
{{.include greeting.md name=visitor}}    <!-- resolves `visitor` from the caller's scope -->
{{.include greeting.md name}}            <!-- shorthand: name=name, resolves `name` from caller's scope -->
```

A bareword argument with no `key=` (`{{.include greeting.md name}}`) is
shorthand for `name=name`: it looks up `name` in the caller's scope and
passes it under a parameter of the same name. Any argument value — literal
or variable — may be piped through the same filters as `.var`.

### Styling: tag/block classes and Tailwind

`RenderOptions.tags` and `.blocks` (plus their per-page front-matter
overrides) let you attach CSS classes to Markdown's own generated elements
without a custom renderer:

```rust
options.tags.insert("h1".into(), "text-4xl font-bold".into());
options.tags.insert("code".into(), "overflow-x-auto".into());
```

Supported tag keys: `h1`–`h6`, `p`, `blockquote`, `code` (fenced code
blocks — the language, if any, is kept as `class="language-<lang>"` on the
inner `<code>`), `ul`, `ol` (ordered lists also keep their `start="N"`
attribute), and `li`.

`compile_styles()` shells out to a `tailwindcss` binary (overridable via the
`TAILWIND_BIN` env var, default: `tailwindcss` on `PATH`) to compile
`style_path` into `<public_dir>/assets/style.css`.

### Errors

All fallible operations return `render_md::Error`, a thin wrapper around
`ErrorKind` (see [`crates/render_md/src/error.rs`](crates/render_md/src/error.rs)) covering things like
`VariableNotFound`, `IncludeFileNotFound`, `IncludePathTraversal`,
`MaxRecursionDepthExceeded`, `UnknownDirective`, and `TailwindCompilationFailed`.

`RenderEngine::format_error` renders an error to a string — plain
`Display` output by default, or (with the `detailed-errors` feature enabled
and `RenderOptions.detailed_errors` set) a `miette` report with source
snippets and a `help` hint for the specific failure.

### Safety notes

- `.include` paths are resolved relative to the including file and then
  checked against `src_dir` — attempts to escape it via `..` are rejected
  as `IncludePathTraversal`, whether the traversal happens in a plain
  `.include` path or in the resolved route from `resolve_paths`.
- `max_include_depth` (default `10`, see `MAX_INCLUDE_DEPTH_DEFAULT`) bounds
  recursion through both plain includes and includes reachable only via
  interpolated argument/condition literals, so circular includes fail
  deterministically instead of overflowing the stack.

## render_md_cgi

[`crates/render_md_cgi`](crates/render_md_cgi) builds a `render_md` binary
that reads its configuration from CGI environment variables, renders (or
serves the cached, non-stale copy of) the requested page, and prints an
HTTP response to stdout — meant to sit behind something like `fcgiwrap`.

Its `render_md_cgi` library exposes the pieces the binary is built from —
`engine_from_env` (reads `APP_ROOT`/`DOCUMENT_TITLE`/`MAX_INCLUDE_DEPTH`/
`DETAILED_ERRORS` into a configured `RenderEngine`), `serve` (resolves the
requested URI, compiles if stale, and prints the CGI response, falling back
to a `_404`/`_500` page on error), and `handle_error`/`serve_file` as the
smaller pieces `serve` is built from — so anything embedding the same
behavior (like the `cgi_bin` example below, which only differs in what root
it defaults to) doesn't have to reimplement it.

See [`crates/render_md_cgi/examples/cgi_bin`](crates/render_md_cgi/examples/cgi_bin)
for a full worked example: sample pages, a Tailwind+daisyUI template, an
nginx/fastcgi config, and a deploy script.

```bash
# run the example from the repo root
cargo run --example cgi_bin -p render_md_cgi
```

## render_md_compile

[`crates/render_md_compile`](crates/render_md_compile) builds `compile_md`,
a `clap`-driven CLI that walks a source directory and renders every page
(every `index.md`) plus the stylesheet up front, instead of lazily on
request the way `render_md_cgi` does — meant for a build step or CI. It
keeps going after a page fails so every page gets a chance to compile,
prints progress to stdout and failures to stderr, and exits non-zero if
anything failed.

```text
Usage: compile_md [OPTIONS]

Options:
      --src-dir <SRC_DIR>
          Directory containing markdown pages; each route is an `index.md`
          (relative to `--base-dir`, unless absolute) [default: src]
      --public-dir <PUBLIC_DIR>
          Directory the compiled static output is written to
          (relative to `--base-dir`, unless absolute) [default: public]
      --template <TEMPLATE>
          Path to the shared HTML template
          (relative to `--base-dir`, unless absolute) [default: src/template.html]
      --style <STYLE>
          Path to the Tailwind stylesheet entry point
          (relative to `--base-dir`, unless absolute) [default: src/styles/tailwind.css]
      --base-dir <BASE_DIR>
          Base directory the other paths are resolved against, and that
          Tailwind is invoked from [default: .]
      --title <TITLE>
          Fallback page title, used unless a page's front matter sets its own
      --max-include-depth <MAX_INCLUDE_DEPTH>
          Maximum recursion depth for `{{.include ...}}` directives [default: 10]
      --detailed-errors
          Emit miette-formatted, source-mapped errors (only present when
          built with the `detailed-errors` feature)
  -h, --help                     Print help
  -V, --version                  Print version
```

`--base-dir` is canonicalized before the other paths are resolved against
it, since `compile_styles` also runs Tailwind with its working directory
set to `base_dir` — so a relative `--base-dir` doesn't get resolved twice.

```bash
# compile the cgi_bin example's site into ./out
cargo run -p render_md_compile --bin compile_md -- \
  --base-dir crates/render_md_cgi/examples/cgi_bin \
  --public-dir out
```

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo test -p render_md --features detailed-errors
```

A [Nix flake](flake.nix) is provided for a reproducible dev shell
(`nix develop`) with the Rust toolchain, `tailwindcss`, and `biome` (used to
format/lint the JS/CSS under [`crates/render_md_cgi/examples`](crates/render_md_cgi/examples)).

## License

MIT
