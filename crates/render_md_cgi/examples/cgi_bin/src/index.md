---
title: render_md
tags:
    ol: list-decimal pl-10
    ul: pl-6
blocks:
    tip: alert my-6 block whitespace-pre shadow-md
    filler: pb-4
tagline: a tiny templating engine for turning markdown into a static, or lazily-rendered, site
status: online
greeter: friend
---

# {{.var title}}

This page *is* the demo: everything below is rendered from this file,
`src/index.md`, by the very engine it describes. See [[about|the about
page]] for what `render_md` actually is — this is a tour of what it can do,
including a [full Markdown tour](#markdown-tour) further down.

## Variables and filters

Front matter can declare arbitrary custom variables, interpolated with
`\{{.var}}` and optionally piped through filters (`upper`, `lower`, `trim`,
`title`):

- Plain:
{{.block tip}}
`\{{.var tagline}}` *becomes* {{.var tagline}}
{{.end}}
- Uppercased:
{{.block tip}}
`\{{.var tagline | upper}}` *becomes* {{.var tagline | upper}}
{{.end}}
- Title-cased:
{{.block tip}}
`\{{.var tagline | title}}` *becomes* {{.var tagline | title}}
{{.end}}

## Conditionals

`\{{.if}}` / `\{{.else}}` / `\{{.end}}` branch on a variable's truthiness, or on a
comparison against a literal or another variable:

{{.block tip}}
  \{{.if status == "online"}}
  🟢 Status: \*\*\{{.var status}}\*\*
  \{{.else}}
  🔴 Status: offline
  \{{.end}}
{{.end}}

becomes

{{.block tip}}
  {{.if status == "online"}}
  🟢 Status: **{{.var status}}**
  {{.else}}
  🔴 Status: offline
  {{.end}}
{{.end}}

You can also check if a variable is set:

{{.block tip}}
  `\{{.if !nonexistent}}`
  A missing variable in a condition is just falsy rather than an error,
  unlike `\{{.var}}` — `nonexistent` was never set, and this whole paragraph
  only exists because `\{{.if !nonexistent}}` evaluated true.
  `\{{.end}}`

  `\{{.if nonexistent}}`
  This paragraph will never show.
  `\{{.end}}`
{{.end}}

becomes

{{.block tip}}
  {{.if !nonexistent}}
  A missing variable in a condition is just falsy rather than an error,
  unlike `\{{.var}}` — `nonexistent` was never set, and this whole paragraph
  only exists because `\{{.if !nonexistent}}` evaluated true.
  {{.end}}

  {{.if nonexistent}}
  This paragraph will never show.
  {{.end}}
{{.end}}

## Styled blocks

`\{{.block name}}...\{{.end}}` wraps its content in a `<div>`, resolving
`name` against this page's own `blocks` front matter — or falling back to
using `name` itself as the class list if it isn't mapped:

{{.block tip}}
  `\{{.block tip}}`
  This callout is a `\{{.block tip}}...\{{.end}}`, not hand-written HTML —
  its class comes from this page's front matter.
  `\{{.end}}`
{{.end}}

becomes

{{.block tip}}
This callout is a `\{{.block tip}}...\{{.end}}`, not hand-written HTML —
its class comes from this page's front matter.
{{.end}}

## Reusable components

Any included file can declare its own `args` schema in front matter.
`about/greeter.md` requires a `greeter` argument, and is included here a
few different ways:

- Shorthand — resolves `greeter` from this page's own scope (set to
  "friend" above):  

{{.block tip}}
  `\{{.include about/greeter.md greeter}}`
{{.end}}

  becomes

{{.block tip}}
  {{.include about/greeter.md greeter}}  
{{.end}}

- Quoted literal:

{{.block tip}}
  `\{{.include about/greeter.md greeter="World"}}`
{{.end}}

becomes

{{.block tip}}
  {{.include about/greeter.md greeter="World"}}
{{.end}}

- A literal with a nested directive inside it:

{{.block tip}}
  `\{{.include about/greeter.md greeter="{{.var tagline | title}}"}}`
{{.end}}

becomes

{{.block tip}}
  {{.include about/greeter.md greeter="{{.var tagline | title}}"}}
{{.end}}

- A variable, filtered before it's passed along:

{{.block tip}}
  `\{{.include about/greeter.md greeter=status | upper}}`
{{.end}}

becomes

{{.block tip}}
  {{.include about/greeter.md greeter=status | upper}}
{{.end}}

## Markdown, in full {#markdown-tour}

Tables, footnotes, strikethrough, task lists, and wikilinks are all
enabled ([pulldown-cmark](https://crates.io/crates/pulldown-cmark)
extensions), alongside the tag classes from this page's own front matter:

1. Ordered lists get `list-decimal pl-10` here
2. Unordered lists get `pl-6`

- [x] Ship the engine
- [ ] ~~Write more lorem ipsum~~

| Directive | Purpose |
| --- | --- |
| `.var` | interpolate a variable, optionally filtered |
| `.include` | inline another file, optionally with arguments |
| `.if` / `.block` | branch, or wrap content in styled markup |

> Blockquotes, like this one, get their own base styling from
> `styles/tailwind.css` regardless of any `tags` override.

Rendering failures are first-class too[^errors].

[^errors]: Spanned `render_md::Error`s, optionally `miette`-formatted with
    source snippets behind the `detailed-errors` feature — see the meta
    section below.

## Meta: even the error pages are just includes

`_404.md` and `_500.md` aren't special-cased by `render_md` — they're
ordinary components with their own `args` schema. Included here to prove
it, not because anything's actually broken:

{{.block tip}}
  {{.include _404.md}}
{{.end}}

`_500.md` declares `error: "An unknown error occurred"` as a *default*
argument, unlike `greeter.md`'s *required* one above — omitted here, so
the default renders:

{{.block tip}}
  {{.include _500.md}}
{{.end}}

{{.block filler}}{{.end}}

You can also see [a dedicated error page here](/error)

---

The boilerplate code of this website is quite little:

```rust
// crates/render_md_compile drives the same RenderEngine this page does,
// just ahead of time instead of on request.
let engine = RenderEngine::<gray_matter::engine::YAML>::new(paths, options);
engine.compile_page(&src_md, &public_html, &mut vars)?;
```

{{.include syntax-highlighting.html}}
