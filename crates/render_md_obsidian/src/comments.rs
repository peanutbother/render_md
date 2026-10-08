//! Step 2: strip Obsidian comments (`%% … %%`), inline and block.
//!
//! A comment beats a code fence: the kanban plugin stores its settings as
//! `%% kanban:settings`, a fenced JSON block and a closing `%%`, and all of
//! it is hidden. A fence (or inline code span) that starts outside a
//! comment protects any `%%` inside it.

use crate::text::{line_end, line_start, parser_options};
use pulldown_cmark::{Event, Parser, Tag};
use std::ops::Range;

pub fn strip_comments(text: &str) -> String {
    if !text.contains("%%") {
        return text.to_owned();
    }
    let code = code_ranges(text);
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;

    while let Some(start) = find_outside_code(text, cursor, &code) {
        // The closing `%%` is searched without regard to code: inside a
        // comment, a fence is just commented-out text.
        let end = text[start + 2..]
            .find("%%")
            .map_or(text.len(), |i| start + 2 + i + 2);
        let (cut_start, cut_end) = widen_to_whole_lines(text, start, end);
        out.push_str(&text[cursor..cut_start]);
        cursor = cut_end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Ranges of fenced/indented code blocks and inline code spans.
fn code_ranges(text: &str) -> Vec<Range<usize>> {
    Parser::new_ext(text, parser_options())
        .into_offset_iter()
        .filter_map(|(event, range)| match event {
            Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => Some(range),
            _ => None,
        })
        .collect()
}

fn find_outside_code(text: &str, from: usize, code: &[Range<usize>]) -> Option<usize> {
    let mut search = from;
    while let Some(i) = text[search..].find("%%") {
        let pos = search + i;
        match code.iter().find(|r| r.contains(&pos)) {
            Some(r) => search = r.end.max(pos + 1),
            None => return Some(pos),
        }
    }
    None
}

/// A comment that is alone on its lines takes the lines with it, so it
/// doesn't leave a blank line behind (which would split a list or a
/// paragraph in two).
fn widen_to_whole_lines(text: &str, start: usize, end: usize) -> (usize, usize) {
    let first = line_start(text, start);
    let last = line_end(text, end);
    let alone = text[first..start].trim().is_empty() && text[end..last].trim().is_empty();
    if !alone {
        return (start, end);
    }
    let last = if last < text.len() { last + 1 } else { last };
    (first, last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inline_comment_is_removed() {
        assert_eq!(strip_comments("a %%hidden%% b"), "a  b");
    }

    #[test]
    fn test_comment_containing_a_fence_is_removed() {
        let text = "## Done\n\n%% kanban:settings\n```\n{\"kanban-plugin\":\"board\"}\n```\n%%\n";
        assert_eq!(strip_comments(text), "## Done\n\n");
    }

    #[test]
    fn test_fence_protects_percent_signs() {
        let text = "```\n%% not a comment %%\n```\n";
        assert_eq!(strip_comments(text), text);
    }

    #[test]
    fn test_inline_code_protects_percent_signs() {
        let text = "use `%%` here and %%drop%% this";
        assert_eq!(strip_comments(text), "use `%%` here and  this");
    }

    #[test]
    fn test_unclosed_comment_runs_to_the_end() {
        assert_eq!(strip_comments("keep\n%% open\nrest"), "keep\n");
    }

    #[test]
    fn test_whole_line_comment_leaves_no_blank_line() {
        assert_eq!(
            strip_comments("- one\n%% hidden %%\n- two\n"),
            "- one\n- two\n"
        );
    }
}
