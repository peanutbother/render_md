//! Step 4: callouts. A blockquote whose first line is `> [!type]fold title`
//! becomes a `<details>` (fold `+` open, `-` closed) or a `<div>` (no fold
//! marker), with the quoted lines, unquoted once, as Markdown inside.
//!
//! Blockquote extents come from the parser, so lazy continuation lines,
//! fences inside the quote and callouts inside list items behave exactly as
//! they do for every other blockquote. Nested callouts are handled by
//! recursing into the unquoted content.

use crate::text::{
    continuation_prefix, escape_html, indent_continuation, line_end, line_start, parser_options,
    render_inline, splice, strip_container_prefix,
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::ops::Range;

pub fn transform(text: &str) -> String {
    if !text.contains("[!") {
        return text.to_owned();
    }
    let mut edits = Vec::new();
    // One entry per open blockquote: whether it is a callout we replace
    // (its nested blockquotes are handled by the recursion).
    let mut open: Vec<bool> = Vec::new();
    for (event, range) in Parser::new_ext(text, parser_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_)) => {
                let replaced = if open.contains(&true) {
                    None
                } else {
                    Callout::parse(text, range.clone()).map(|c| (range.clone(), c.render()))
                };
                open.push(replaced.is_some());
                edits.extend(replaced);
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                open.pop();
            }
            _ => {}
        }
    }
    splice(text, edits)
}

struct Callout {
    kind: String,
    fold: Option<char>,
    title: String,
    body: String,
    /// Prefix for every line after the first (container markers).
    prefix: String,
    /// Whether to put a blank line before the block.
    blank_before: bool,
    /// Whether the replaced range ended with a newline.
    trailing_newline: bool,
}

impl Callout {
    fn parse(text: &str, range: Range<usize>) -> Option<Self> {
        let header_end = line_end(text, range.start).min(range.end);
        let header = text[range.start..header_end]
            .strip_prefix('>')?
            .trim_start();
        let rest = header.strip_prefix("[!")?;
        let close = rest.find(']')?;
        let kind = &rest[..close];
        if kind.is_empty()
            || !kind
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return None;
        }
        let mut after = &rest[close + 1..];
        let fold = after.chars().next().filter(|c| matches!(c, '+' | '-'));
        if fold.is_some() {
            after = &after[1..];
        }

        let before = &text[line_start(text, range.start)..range.start];
        let prefix = continuation_prefix(before);
        let content = text.get(header_end + 1..range.end).unwrap_or_default();
        let content = content.strip_suffix('\n').unwrap_or(content);
        let body: Vec<&str> = if content.is_empty() {
            Vec::new()
        } else {
            content
                .split('\n')
                .map(|line| unquote(strip_container_prefix(line, &prefix)))
                .collect()
        };

        Some(Self {
            kind: kind.to_lowercase(),
            fold,
            title: after.trim().to_owned(),
            body: transform(&body.join("\n")),
            prefix,
            blank_before: before.trim().is_empty(),
            trailing_newline: text[..range.end].ends_with('\n'),
        })
    }

    fn render(&self) -> String {
        let title = if self.title.is_empty() {
            capitalize(&self.kind)
        } else {
            render_inline(&self.title)
        };
        let kind = escape_html(&self.kind);
        let (open, title, close) = match self.fold {
            Some(fold) => (
                format!(
                    "<details class=\"callout\" data-callout=\"{kind}\"{}>",
                    if fold == '+' { " open" } else { "" }
                ),
                format!("<summary class=\"callout-title\">{title}</summary>"),
                "</details>",
            ),
            None => (
                format!("<div class=\"callout\" data-callout=\"{kind}\">"),
                format!("<div class=\"callout-title\">{title}</div>"),
                "</div>",
            ),
        };

        let mut block = format!("{open}\n{title}\n");
        let body = self.body.trim_matches('\n');
        if !body.trim().is_empty() {
            block.push_str("<div class=\"callout-content\">\n\n");
            block.push_str(body);
            block.push_str("\n\n</div>\n");
        }
        block.push_str(close);

        let mut out = String::new();
        if self.blank_before {
            out.push('\n');
            out.push_str(&self.prefix);
        }
        out.push_str(&indent_continuation(&block, &self.prefix));
        if self.trailing_newline {
            // A blank line after the block ends the HTML block, so whatever
            // follows is Markdown again.
            out.push('\n');
            out.push_str(self.prefix.trim_end());
            out.push('\n');
        }
        out
    }
}

/// Removes one level of blockquote marker (`>` and one optional space). A
/// line without one is a lazy continuation line and stays as it is.
fn unquote(line: &str) -> &str {
    let trimmed = line.trim_start_matches(' ');
    match trimmed.strip_prefix('>') {
        Some(rest) if line.len() - trimmed.len() <= 3 => rest.strip_prefix(' ').unwrap_or(rest),
        _ => line,
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(markdown: &str) -> String {
        let mut out = String::new();
        pulldown_cmark::html::push_html(
            &mut out,
            Parser::new_ext(&transform(markdown), parser_options()),
        );
        out
    }

    #[test]
    fn test_plain_callout_becomes_div() {
        let out = transform("> [!tip] dry run\n> body *text*\n");
        assert_eq!(
            out,
            "\n<div class=\"callout\" data-callout=\"tip\">\n<div class=\"callout-title\">dry run</div>\n<div class=\"callout-content\">\n\nbody *text*\n\n</div>\n</div>\n\n"
        );
    }

    #[test]
    fn test_fold_markers_and_empty_title() {
        let open = transform("> [!example]+ Checklist\n> 1. one\n");
        assert!(open.contains("<details class=\"callout\" data-callout=\"example\" open>"));
        assert!(open.contains("<summary class=\"callout-title\">Checklist</summary>"));
        let closed = transform("> [!NOTE]-\n> x\n");
        assert!(closed.contains("<details class=\"callout\" data-callout=\"note\">"));
        assert!(closed.contains(">Note</summary>"));
    }

    #[test]
    fn test_callout_with_list_and_indented_fence() {
        let src = "> [!example]+ Steps\n> 1. **Roll out**:\n>    ```yaml\n>    key: [a]\n>    ```\n> 2. Check\n";
        let out = html(src);
        assert!(out.contains("<ol>"), "{out}");
        assert!(
            out.contains("<pre><code class=\"language-yaml\">key: [a]\n</code></pre>"),
            "{out}"
        );
        assert!(out.contains("Check"), "{out}");
        assert!(!out.contains("[!"), "{out}");
    }

    #[test]
    fn test_nested_plain_quote_and_nested_callout() {
        let out = html("> [!fix] #fix\n> > quoted\n>\n> > [!tip] inner\n> > text\n");
        assert!(
            out.contains("<blockquote>\n<p>quoted</p>\n</blockquote>"),
            "{out}"
        );
        assert!(out.contains("data-callout=\"tip\""), "{out}");
        assert!(!out.contains("[!"), "{out}");
    }

    #[test]
    fn test_plain_blockquote_is_untouched() {
        assert_eq!(
            transform("> just [!not] a callout\n"),
            "> just [!not] a callout\n"
        );
        assert_eq!(transform("> quote\n"), "> quote\n");
    }

    #[test]
    fn test_lazy_continuation_line_stays_in_callout() {
        let out = html("> [!tip] t\n> first\nlazy\n\nafter\n");
        let content = out.split("</div>\n</div>").next().unwrap();
        assert!(content.contains("lazy"), "{out}");
        assert!(out.contains("<p>after</p>"), "{out}");
    }

    #[test]
    fn test_callout_in_list_item() {
        let out = html("- item\n\n  > [!note] t\n  > body\n- next\n");
        assert!(
            out.contains("<li>\n<p>item</p>\n<div class=\"callout\""),
            "{out}"
        );
        assert!(
            out.contains("<p>next</p>") || out.contains("<li>next"),
            "{out}"
        );
    }

    #[test]
    fn test_heading_right_after_callout_is_still_a_heading() {
        let out = html("> [!warning] careful\n> text\n# Health\n");
        assert!(out.contains("<h1>Health</h1>"), "{out}");
    }
}
