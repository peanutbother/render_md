---
title: About
---

# About render_md

This site runs on [`render_md`](https://crates.io/crates/render_md), a
small Rust library for turning a directory of Markdown files into a static
site — or a lazily-rendered, cached one — with a minimal templating
language for variables, includes, conditionals, and styled blocks, built on
top of [`pulldown-cmark`](https://crates.io/crates/pulldown-cmark).

The workspace also ships `render_md_cgi` (the CGI binary and example this
page runs as) and `render_md_compile`, a `clap`-driven CLI that compiles a
whole site ahead of time instead of lazily per request.

None of it depends on a database or a build step: pages compile to HTML on
first request and are served from the cache after that, until a source
file changes.

**[View the source →](https://github.com/peanutbother/render_md)**

Curious what the templating language and Markdown support actually look
like? The [home page](/) is a full tour, rendered by the same engine.
