use crate::Error;
use miette::{NamedSource, SourceSpan};
use std::path::Path;

/// Represents a parsed directive from a template tag.
#[derive(Debug, PartialEq, Eq)]
pub enum Directive<'a> {
    /// A variable interpolation directive, e.g., `{{.var my_var}}`
    Var(&'a str),
    /// A file inclusion directive, e.g., `{{.include my_file.md arg="value"}}`
    Include { file: &'a str, args: &'a str },
    /// A conditional directive with a raw, unparsed condition, e.g.,
    /// `{{.if my_var}}` or `{{.if my_var == "value"}}`
    If(&'a str),
    /// Switches to the else branch of an `.if`, e.g., `{{.else}}`
    Else,
    /// A block container directive, e.g., `{{.block hero}}`
    Block(&'a str),
    /// Closes a block container or an `.if`, e.g., `{{.end}}`
    End,
}

/// A matched template tag in source text with its span and byte offsets
#[derive(Debug)]
pub struct TagMatch<'a> {
    pub directive: Directive<'a>,
    pub span: SourceSpan,
    pub match_start: usize,
    pub match_end: usize,
}

/// Scanner responsible for finding and lexing template tags
#[derive(Debug)]
pub struct TagScanner<'a> {
    pub src: &'a str,
    pub named_src: NamedSource<String>,
}

impl<'a> TagScanner<'a> {
    /// Creates a new `TagScanner` for a given template string and file path.
    pub fn new(src: &'a str, file_path: &Path) -> Self {
        Self {
            src,
            named_src: NamedSource::new(file_path.display().to_string(), src.to_string()),
        }
    }

    /// Finds the next directive tag in input starting at `offset`
    pub fn next_tag(&self, offset: usize) -> Result<Option<TagMatch<'a>>, Error> {
        let remaining = &self.src[offset..];
        let Some(rel_start) = remaining.find("{{.") else {
            return Ok(None);
        };

        let start_pos = offset + rel_start;
        let tag_body = &self.src[start_pos + 3..];

        let Some(rel_end) = find_tag_end(tag_body) else {
            let span = SourceSpan::new(start_pos.into(), self.src.len() - start_pos);
            return Err(Error::unclosed_tag(self.named_src.clone(), span));
        };

        let end_pos = start_pos + 3 + rel_end + 2;
        let span = SourceSpan::new(start_pos.into(), end_pos - start_pos);
        let directive_str = tag_body[..rel_end].trim();

        let directive = self.parse_directive(directive_str, span)?;

        Ok(Some(TagMatch {
            directive,
            span,
            match_start: start_pos,
            match_end: end_pos,
        }))
    }

    fn parse_directive(&self, text: &'a str, span: SourceSpan) -> Result<Directive<'a>, Error> {
        if let Some(var_name) = text.strip_prefix("var ") {
            Ok(Directive::Var(var_name.trim()))
        } else if let Some(include_str) = text.strip_prefix("include ") {
            let mut parts = include_str.trim().splitn(2, |c: char| c.is_whitespace());
            let file = parts.next().unwrap_or("").trim();
            let args = parts.next().unwrap_or("").trim();
            if file.is_empty() {
                return Err(Error::missing_include_name(self.named_src.clone(), span));
            }
            Ok(Directive::Include { file, args })
        } else if text == "block" {
            Err(Error::missing_block_name(self.named_src.clone(), span))
        } else if let Some(name) = text.strip_prefix("block ") {
            let name = name.trim();
            if name.is_empty() {
                Err(Error::missing_block_name(self.named_src.clone(), span))
            } else {
                Ok(Directive::Block(name))
            }
        } else if text == "end" {
            Ok(Directive::End)
        } else if text == "if" {
            Err(Error::missing_if_condition(self.named_src.clone(), span))
        } else if let Some(cond) = text.strip_prefix("if ") {
            let cond = cond.trim();
            if cond.is_empty() {
                Err(Error::missing_if_condition(self.named_src.clone(), span))
            } else {
                Ok(Directive::If(cond))
            }
        } else if text == "else" {
            Ok(Directive::Else)
        } else {
            Err(Error::unknown_directive(
                text.to_string(),
                self.named_src.clone(),
                span,
            ))
        }
    }
}

/// Finds the byte offset of the `}}` that closes a tag body, ignoring any
/// `}}` that occurs inside a quoted literal (so a nested tag embedded in an
/// include-arg or `.if`-condition literal, e.g.
/// `{{.include f.md title="Hi {{.var name}}"}}`, doesn't prematurely
/// terminate the outer tag at the nested tag's own closing `}}`).
/// Quote-tracking mirrors `args::parse_quoted_literal`'s `\"`/`\\` escaping.
fn find_tag_end(tag_body: &str) -> Option<usize> {
    let bytes = tag_body.as_bytes();
    let mut i = 0;
    let mut in_quotes = false;
    let mut escaped = false;

    while i < bytes.len() {
        let c = bytes[i];
        if in_quotes {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_quotes = false;
            }
            i += 1;
            continue;
        }

        if c == b'"' {
            in_quotes = true;
            i += 1;
        } else if c == b'}' && bytes.get(i + 1) == Some(&b'}') {
            return Some(i);
        } else {
            i += 1;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scanner(src: &str) -> TagScanner<'_> {
        TagScanner::new(src, &PathBuf::from("test.md"))
    }

    #[test]
    fn test_next_tag_returns_none_without_a_tag() {
        let s = scanner("just plain text");
        assert!(s.next_tag(0).unwrap().is_none());
    }

    #[test]
    fn test_next_tag_var_directive() {
        let s = scanner("Hello {{.var name}}!");
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(tag.directive, Directive::Var("name"));
        assert_eq!(tag.match_start, 6);
        assert_eq!(tag.match_end, 19);
    }

    #[test]
    fn test_next_tag_include_directive_with_args() {
        let s = scanner(r#"{{.include ./greeter.md name="Bob"}}"#);
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(
            tag.directive,
            Directive::Include {
                file: "./greeter.md",
                args: r#"name="Bob""#,
            }
        );
    }

    #[test]
    fn test_next_tag_include_directive_without_args() {
        let s = scanner("{{.include ./partial.md}}");
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(
            tag.directive,
            Directive::Include {
                file: "./partial.md",
                args: "",
            }
        );
    }

    #[test]
    fn test_next_tag_bare_include_is_unknown_directive() {
        // With no trailing space, "include" alone doesn't match the
        // `include ` prefix, so it's reported as an unknown directive
        // rather than a missing file name.
        let s = scanner("{{.include}}");
        let err = s.next_tag(0).unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnknownDirective { directive, .. } => {
                assert_eq!(directive, "include")
            }
            _ => panic!("expected UnknownDirective, got {:?}", err),
        }
    }

    #[test]
    fn test_parse_directive_missing_include_name_errors() {
        // The outer `next_tag` scan trims trailing whitespace before it ever
        // reaches `parse_directive`, so an empty file name after the
        // `include ` prefix can't be produced through `next_tag` itself.
        // Exercise `parse_directive` directly to cover that branch.
        let s = scanner("");
        let span = SourceSpan::new(0.into(), 0);
        let err = s.parse_directive("include ", span).unwrap_err();
        assert!(matches!(
            err.inner(),
            crate::ErrorKind::MissingIncludeName { .. }
        ));
    }

    #[test]
    fn test_next_tag_block_directive() {
        let s = scanner("{{.block hero}}");
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(tag.directive, Directive::Block("hero"));
    }

    #[test]
    fn test_next_tag_missing_block_name_errors() {
        let s = scanner("{{.block}}");
        let err = s.next_tag(0).unwrap_err();
        assert!(matches!(
            err.inner(),
            crate::ErrorKind::MissingBlockName { .. }
        ));
    }

    #[test]
    fn test_next_tag_block_with_trailing_space_only_is_missing_name() {
        let s = scanner("{{.block }}");
        let err = s.next_tag(0).unwrap_err();
        assert!(matches!(
            err.inner(),
            crate::ErrorKind::MissingBlockName { .. }
        ));
    }

    #[test]
    fn test_next_tag_blockquote_is_unknown_directive() {
        // Without a word boundary after "block", a tag like `.blockquote`
        // must not be misparsed as `Directive::Block("quote")`.
        let s = scanner("{{.blockquote}}");
        let err = s.next_tag(0).unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnknownDirective { directive, .. } => {
                assert_eq!(directive, "blockquote")
            }
            _ => panic!("expected UnknownDirective, got {:?}", err),
        }
    }

    #[test]
    fn test_next_tag_end_directive() {
        let s = scanner("{{.end}}");
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(tag.directive, Directive::End);
    }

    #[test]
    fn test_next_tag_if_directive() {
        let s = scanner("{{.if flag}}");
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(tag.directive, Directive::If("flag"));
    }

    #[test]
    fn test_next_tag_if_directive_with_comparison() {
        let s = scanner(r#"{{.if a == "b"}}"#);
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(tag.directive, Directive::If(r#"a == "b""#));
    }

    #[test]
    fn test_next_tag_if_negation_directive() {
        let s = scanner("{{.if !flag}}");
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(tag.directive, Directive::If("!flag"));
    }

    #[test]
    fn test_next_tag_missing_if_condition_errors() {
        let s = scanner("{{.if}}");
        let err = s.next_tag(0).unwrap_err();
        assert!(matches!(
            err.inner(),
            crate::ErrorKind::MissingIfCondition { .. }
        ));
    }

    #[test]
    fn test_next_tag_if_with_trailing_space_only_is_missing_condition() {
        let s = scanner("{{.if }}");
        let err = s.next_tag(0).unwrap_err();
        assert!(matches!(
            err.inner(),
            crate::ErrorKind::MissingIfCondition { .. }
        ));
    }

    #[test]
    fn test_parse_directive_missing_if_condition_direct() {
        // Mirrors `test_parse_directive_missing_include_name_errors`: `next_tag`
        // trims the tag body before `parse_directive` runs, so an empty
        // condition after the `if ` prefix can't be produced through
        // `next_tag` itself with multiple trailing spaces. Exercise
        // `parse_directive` directly to cover that branch.
        let s = scanner("");
        let span = SourceSpan::new(0.into(), 0);
        let err = s.parse_directive("if ", span).unwrap_err();
        assert!(matches!(
            err.inner(),
            crate::ErrorKind::MissingIfCondition { .. }
        ));
    }

    #[test]
    fn test_next_tag_ifoo_is_unknown_directive() {
        // Word-boundary regression, mirrors the `.blockquote` test: `.ifoo`
        // must not be misparsed as `If("oo")`.
        let s = scanner("{{.ifoo}}");
        let err = s.next_tag(0).unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnknownDirective { directive, .. } => {
                assert_eq!(directive, "ifoo")
            }
            _ => panic!("expected UnknownDirective, got {:?}", err),
        }
    }

    #[test]
    fn test_next_tag_else_directive() {
        let s = scanner("{{.else}}");
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(tag.directive, Directive::Else);
    }

    #[test]
    fn test_next_tag_elseif_is_unknown_directive() {
        let s = scanner("{{.elseif x}}");
        let err = s.next_tag(0).unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnknownDirective { directive, .. } => {
                assert_eq!(directive, "elseif x")
            }
            _ => panic!("expected UnknownDirective, got {:?}", err),
        }
    }

    #[test]
    fn test_next_tag_include_with_nested_tag_in_literal_arg() {
        // The outer tag's `}}` boundary must not be confused with the
        // nested `{{.var name}}`'s own closing `}}` inside the literal.
        let s = scanner(r#"{{.include card.md title="Hi {{.var name}}"}}"#);
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(
            tag.directive,
            Directive::Include {
                file: "card.md",
                args: r#"title="Hi {{.var name}}""#,
            }
        );
    }

    #[test]
    fn test_next_tag_if_with_nested_tag_in_literal_operand() {
        let s = scanner(r#"{{.if name == "Hi {{.var other}}"}}"#);
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(
            tag.directive,
            Directive::If(r#"name == "Hi {{.var other}}""#)
        );
    }

    #[test]
    fn test_next_tag_var_with_filters_directive() {
        // Filter-splitting happens downstream in evaluator.rs/filters.rs;
        // the lexer sees the whole "name | upper | trim" as one raw string.
        let s = scanner("{{.var name | upper | trim}}");
        let tag = s.next_tag(0).unwrap().unwrap();
        assert_eq!(tag.directive, Directive::Var("name | upper | trim"));
    }

    #[test]
    fn test_next_tag_unknown_directive_errors() {
        let s = scanner("{{.magic spell}}");
        let err = s.next_tag(0).unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnknownDirective { directive, .. } => {
                assert_eq!(directive, "magic spell")
            }
            _ => panic!("expected UnknownDirective"),
        }
    }

    #[test]
    fn test_next_tag_unclosed_tag_errors() {
        let s = scanner("Hello {{.var name");
        let err = s.next_tag(0).unwrap_err();
        assert!(matches!(err.inner(), crate::ErrorKind::UnclosedTag { .. }));
    }

    #[test]
    fn test_next_tag_respects_offset_for_sequential_scanning() {
        let s = scanner("{{.var a}} and {{.var b}}");
        let first = s.next_tag(0).unwrap().unwrap();
        assert_eq!(first.directive, Directive::Var("a"));

        let second = s.next_tag(first.match_end).unwrap().unwrap();
        assert_eq!(second.directive, Directive::Var("b"));
    }
}
