//! Small text helpers shared by the transform passes: HTML and Markdown
//! escaping, the parser options, line arithmetic and splicing.

use pulldown_cmark::{Options, Parser, html};
use std::ops::Range;

/// The same extensions `render_md::markdown::render_markdown` enables, so
/// every offset computed here matches how the staged page is rendered.
pub fn parser_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options.insert(Options::ENABLE_WIKILINKS);
    options
}

/// Escapes text for HTML element content and attribute values.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Backslash-escapes every ASCII punctuation character, so `text` renders
/// literally as Markdown inline content (link text, table cells).
pub fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Renders a single line of Markdown as inline HTML (without the `<p>`
/// wrapper), e.g. a callout title.
pub fn render_inline(markdown: &str) -> String {
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(markdown.trim(), parser_options()));
    let out = out.trim_end();
    out.strip_prefix("<p>")
        .and_then(|s| s.strip_suffix("</p>"))
        .unwrap_or(out)
        .to_owned()
}

/// Byte offset where the line containing `pos` starts.
pub fn line_start(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

/// Byte offset of the `\n` ending the line containing `pos` (or the end of
/// the text).
pub fn line_end(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i)
}

/// The prefix that continuation lines of a block need to stay inside the
/// same containers as its first line. `before` is the text between the
/// start of the block's first line and the block itself: blockquote
/// markers are kept, list markers become spaces (`"- > "` becomes `"  > "`).
pub fn continuation_prefix(before: &str) -> String {
    before
        .chars()
        .map(|c| match c {
            '>' | ' ' | '\t' => c,
            _ => ' ',
        })
        .collect()
}

/// Strips the container prefix `prefix` (as built by
/// [`continuation_prefix`]) from a continuation `line`. Spaces in the
/// prefix are matched loosely and a missing `>` (a lazy continuation line)
/// stops the stripping early.
pub fn strip_container_prefix<'a>(line: &'a str, prefix: &str) -> &'a str {
    let mut rest = line;
    for p in prefix.chars() {
        match p {
            '>' => {
                let trimmed = rest.trim_start_matches([' ', '\t']);
                match trimmed.strip_prefix('>') {
                    Some(after) => rest = after,
                    None => return trimmed,
                }
            }
            _ => {
                if let Some(after) = rest.strip_prefix([' ', '\t']) {
                    rest = after;
                }
            }
        }
    }
    rest
}

/// Indents every line of `block` after the first with `prefix`; blank lines
/// get the prefix without trailing whitespace (`>` for a blockquote).
pub fn indent_continuation(block: &str, prefix: &str) -> String {
    let blank = prefix.trim_end();
    let mut out = String::with_capacity(block.len());
    for (i, line) in block.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            if line.trim().is_empty() {
                out.push_str(blank);
            } else {
                out.push_str(prefix);
            }
        }
        out.push_str(line);
    }
    out
}

/// Applies non-overlapping `(range, replacement)` edits to `text`. Edits
/// nested inside an earlier, larger edit are dropped.
pub fn splice(text: &str, mut edits: Vec<(Range<usize>, String)>) -> String {
    // By start; at the same start, insertions first, then the larger edit.
    edits.sort_by(|a, b| {
        a.0.start
            .cmp(&b.0.start)
            .then((!a.0.is_empty()).cmp(&!b.0.is_empty()))
            .then(b.0.end.cmp(&a.0.end))
    });
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for (range, replacement) in edits {
        if range.start < cursor {
            continue;
        }
        out.push_str(&text[cursor..range.start]);
        out.push_str(&replacement);
        cursor = range.end;
    }
    out.push_str(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_markdown_escapes_link_syntax() {
        assert_eq!(escape_markdown("a [b] | c"), r"a \[b\] \| c");
    }

    #[test]
    fn test_render_inline_strips_paragraph() {
        assert_eq!(render_inline("dry *run*"), "dry <em>run</em>");
        assert_eq!(render_inline(""), "");
    }

    #[test]
    fn test_continuation_prefix_turns_list_markers_into_spaces() {
        assert_eq!(continuation_prefix(""), "");
        assert_eq!(continuation_prefix("> "), "> ");
        assert_eq!(continuation_prefix("- > "), "  > ");
        assert_eq!(continuation_prefix("12. "), "    ");
    }

    #[test]
    fn test_strip_container_prefix() {
        assert_eq!(strip_container_prefix("> > x", "> "), "> x");
        assert_eq!(strip_container_prefix("   > x", "   "), "> x");
        assert_eq!(strip_container_prefix("lazy", "> "), "lazy");
    }

    #[test]
    fn test_indent_continuation_uses_bare_marker_on_blank_lines() {
        assert_eq!(indent_continuation("a\n\nb", "> "), "a\n>\n> b");
    }

    #[test]
    fn test_splice_applies_edits_and_drops_nested_ones() {
        let edits = vec![
            (6..11, "there".to_owned()),
            (0..5, "howdy".to_owned()),
            (7..8, "nested".to_owned()),
            (11..11, "!".to_owned()),
            (0..0, "> ".to_owned()),
        ];
        assert_eq!(splice("hello world", edits), "> howdy there!");
    }
}
