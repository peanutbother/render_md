# cgi-bin example

A working example of [`render_md`](../../README.md) driving a small,
Tailwind-styled personal site through classic CGI: nginx hands each request
to a tiny binary via `fcgiwrap`, which renders the requested Markdown page
to HTML (or serves the already-compiled, non-stale copy) and prints it to
stdout as a CGI response.

There's no application server here — `main.rs` runs once per request,
reads a handful of CGI environment variables, and exits.

## Layout

```text
examples/cgi_bin/
├─ src/
│  ├─ main.rs              # the CGI entrypoint
│  ├─ template.html        # shared HTML shell ({{title}}/{{body}} + navbar/footer includes)
│  ├─ styles/
│  │  └─ tailwind.css      # Tailwind entry point (daisyUI plugin)
│  └─ pages/
│     ├─ index.md          # -> route "/"
│     ├─ about/
│     │  ├─ index.md       # -> route "/about"
│     │  └─ greeter.md     # a parameterized component, included by about/index.md
│     ├─ navbar.html        # partial, included from template.html
│     ├─ footer.html        # partial, included from template.html
│     ├─ syntax-highlighting.html
│     ├─ _404.md            # rendered on unresolved routes
│     └─ _500.md            # rendered on a render/compile error
├─ static/                 # copied verbatim into public/assets by build.sh
│  ├─ logo.svg
│  └─ code.css
├─ config/
│  └─ nginx.conf           # example nginx + fastcgi vhost
├─ tailwind.config.js
└─ build.sh                # builds the binary and compiles assets into /var/www
```

`_404.md` and `_500.md` are conventions used by `main.rs`, not something
`render_md` itself special-cases — the app renders them itself when
`resolve_paths` fails or a render/compile step errors.

## How a request is served

1. `DOCUMENT_URI` (e.g. `/about`) is resolved with `engine.resolve_paths`
   against `src/pages`. If nothing matches, `_404.md` is rendered and
   returned with a `404` status.
2. `engine.is_stale` compares the cached `public/<route>/index.html` against
   the template, stylesheet, and every markdown/HTML file under the route's
   directory. If nothing changed, the cached file is served as-is.
3. Otherwise the page is recompiled with `compile_page` and the Tailwind
   stylesheet is rebuilt with `compile_styles`, before serving the fresh
   output.
4. Any error along the way (missing variable, bad include, Tailwind
   failure, ...) is caught, formatted with `engine.format_error`, and
   rendered through `_500.md` with a `500` status instead of crashing the
   CGI process.

Because compiled pages are cached under `public/` and only rebuilt when
stale, most requests are just a stat + file read, not a full Markdown
render.

## Configuration (CGI environment variables)

Set by the web server / fastcgi config (see [`config/nginx.conf`](config/nginx.conf)):

| Variable | Required | Purpose |
| --- | --- | --- |
| `DOCUMENT_URI` | no (defaults to `/`) | The route to render, e.g. `/about`. |
| `APP_ROOT` | no (defaults to `.`) | Base directory containing `src/` and `public/`. |
| `DOCUMENT_TITLE` | no | Fallback page title, used unless a page's front matter sets its own `title`. |
| `MAX_INCLUDE_DEPTH` | no (defaults to `10`) | Overrides the include/recursion depth limit. |
| `DETAILED_ERRORS` | no | `1`/`yes`/`true`/`enable` (case-insensitive) enables `miette`-formatted errors. Only has an effect when the binary is built with `--features detailed-errors`. |
| `TAILWIND_BIN` | no (defaults to `tailwindcss` on `PATH`) | Path to the `tailwindcss` binary used by `compile_styles`. |

## Building & deploying

```bash
./build.sh
```

This builds the release binary, copies it (and a `tailwindcss` binary,
referenced by nginx's `TAILWIND_BIN`) into `/var/www/cgi-bin`, copies
`static/*` into `/var/www/public/assets`, and does an initial Tailwind
compile of `/var/www/public/assets/style.css`. It assumes an `APP_ROOT` of
`/var/www`, matching [`config/nginx.conf`](config/nginx.conf).

For detailed, source-mapped error pages in production, build with:

```bash
cargo build --release --features detailed-errors
```

and set `DETAILED_ERRORS true` in the fastcgi config, as
[`config/nginx.conf`](config/nginx.conf) already does.

### Wiring it into nginx

[`config/nginx.conf`](config/nginx.conf) shows the expected setup:

- `/assets/` is served directly by nginx, bypassing the CGI binary
  entirely (`static/*` + the compiled `style.css` live there).
- Everything else is passed to `fcgiwrap` (`fastcgi_pass
  unix:/run/fcgiwrap.socket`), which invokes the built binary as
  `SCRIPT_FILENAME`, forwarding `DOCUMENT_URI` and the other variables
  above as fastcgi params.

Adjust the socket path, `APP_ROOT`, and `DOCUMENT_TITLE` for your own
deployment.

## Adding a page

1. Create `src/pages/<route>/index.md` (front matter is optional — see the
   [main README's front matter section](../../README.md#front-matter)).
2. Link to it from `navbar.html` if it should appear in navigation.
3. Reference shared pieces (e.g. `syntax-highlighting.html`) or
   parameterized components (like `greeter.md` with `{{.include ...}}`).

The next request to that route compiles and caches it automatically — there
is no separate build step for content changes, only for the Tailwind
stylesheet if you introduce classes that need scanning (`tailwind.config.js`
already scans `src/template.html` and `src/pages/**/*.md`).

## Note on this example crate

This crate is a standalone binary (package `example_page`) that depends on
`render_md`. If you're building it outside of a setup that already wires
that dependency in, add it to [`Cargo.toml`](Cargo.toml):

```toml
[dependencies]
render_md = "..."
http = "1.5.0"
```
