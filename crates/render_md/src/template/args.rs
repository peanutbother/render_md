use std::collections::HashMap;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Argument<'a> {
    /// A quoted literal value, plus any `| filter` chain applied to it.
    Literal(String, Vec<&'a str>),
    /// A variable reference (resolved from the caller's scope), plus any
    /// `| filter` chain applied to its resolved value.
    Variable(&'a str, Vec<&'a str>),
}

/// Parses a quoted literal starting at `s[0] == '"'`, unescaping `\"`/`\\`.
/// Returns the unescaped value, the remainder of `s` after the closing quote
/// (whitespace-trimmed; empty if the quote was never closed), and whether
/// the quote was actually closed. Callers decide how to treat an unterminated
/// quote: `parse_args` tolerates it (captures the partial value), while
/// `condition::parse_condition` treats it as a hard parse error.
pub(crate) fn parse_quoted_literal(s: &str) -> (String, &str, bool) {
    debug_assert!(s.starts_with('"'));

    let mut value = String::new();
    let mut end_bytes = 1; // Includes opening quote
    let mut escaped = false;
    let mut closed = false;

    for c in s[1..].chars() {
        end_bytes += c.len_utf8();
        if escaped {
            value.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            closed = true;
            break;
        } else {
            value.push(c);
        }
    }

    let rest = if closed && end_bytes <= s.len() {
        s[end_bytes..].trim_start()
    } else {
        ""
    };

    (value, rest, closed)
}

/// Consumes a `| filter_name` chain (e.g. `| upper | trim`) from the start
/// of `s`, tolerating whitespace around each `|` (so both `name|upper` and
/// `name | upper` work). Returns the filter names found, in order, and the
/// remainder of `s` after the last filter (whitespace-trimmed).
pub(crate) fn parse_filter_chain(mut s: &str) -> (Vec<&str>, &str) {
    let mut filters = Vec::new();

    loop {
        let trimmed = s.trim_start();
        let Some(after_pipe) = trimmed.strip_prefix('|') else {
            s = trimmed;
            break;
        };
        let after_pipe = after_pipe.trim_start();
        let end = after_pipe
            .find(|c: char| c.is_whitespace() || c == '|')
            .unwrap_or(after_pipe.len());
        let filter_name = &after_pipe[..end];
        if !filter_name.is_empty() {
            filters.push(filter_name);
        }
        s = &after_pipe[end..];
    }

    (filters, s)
}

/// Parses argument strings into a map of keys to Argument values.
/// Handles literals (e.g. key="val\"") and variables (e.g. key=my_var).
///
/// A bareword token with no `=` (e.g. `my_var`) is shorthand for
/// `my_var=my_var`: it's treated as `Argument::Variable` under a key of the
/// same name, so it looks up `my_var` from the caller's scope and binds it
/// to a parameter also named `my_var`.
///
/// Any value (literal or variable, including the bareword-shorthand form)
/// may be followed by a `| filter` chain, e.g. `key=my_var | upper` or the
/// shorthand `my_var|upper`, mirroring `{{.var name | upper}}`'s syntax.
/// Filters are applied to the value once it's resolved (see
/// `fs_env::resolve_include_args`).
pub fn parse_args(mut s: &str) -> HashMap<&str, Argument<'_>> {
    let mut args = HashMap::new();

    while !s.is_empty() {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }

        let ws_idx = s.find(|c: char| c.is_whitespace());
        let eq_idx = s.find('=');

        // If the current token has no `=` before the next whitespace (or
        // end of string), it's a bareword shorthand: `name` -> `name=name`.
        let eq_idx = match (eq_idx, ws_idx) {
            (Some(e), Some(w)) if e < w => e,
            (Some(e), None) => e,
            _ => {
                let end_idx = s
                    .find(|c: char| c.is_whitespace() || c == '|')
                    .unwrap_or(s.len());
                let name = &s[..end_idx];
                let (filters, rest) = parse_filter_chain(&s[end_idx..]);
                args.insert(name, Argument::Variable(name, filters));
                s = rest;
                continue;
            }
        };

        let key = s[..eq_idx].trim();
        s = s[eq_idx + 1..].trim_start();

        if s.starts_with('"') {
            let (value, rest, _closed) = parse_quoted_literal(s);
            let (filters, rest) = parse_filter_chain(rest);
            args.insert(key, Argument::Literal(value, filters));
            s = rest;
        } else {
            let end_idx = s
                .find(|c: char| c.is_whitespace() || c == '|')
                .unwrap_or(s.len());
            let name = &s[..end_idx];
            let (filters, rest) = parse_filter_chain(&s[end_idx..]);
            args.insert(key, Argument::Variable(name, filters));
            s = rest;
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args() {
        let parsed = parse_args(r#"title="Hello\"World" count=my_count"#);
        assert_eq!(
            parsed.get("title"),
            Some(&Argument::Literal("Hello\"World".to_string(), vec![]))
        );
        assert_eq!(
            parsed.get("count"),
            Some(&Argument::Variable("my_count", vec![]))
        );
    }

    #[test]
    fn test_parse_args_empty_string_yields_no_args() {
        let parsed = parse_args("");
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_parse_args_whitespace_only_yields_no_args() {
        let parsed = parse_args("   \t  ");
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_parse_args_single_literal() {
        let parsed = parse_args(r#"name="Bob""#);
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed.get("name"),
            Some(&Argument::Literal("Bob".to_string(), vec![]))
        );
    }

    #[test]
    fn test_parse_args_single_variable() {
        let parsed = parse_args("name=my_name");
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed.get("name"),
            Some(&Argument::Variable("my_name", vec![]))
        );
    }

    #[test]
    fn test_parse_args_tolerates_extra_whitespace_between_pairs() {
        let parsed = parse_args(r#"  a="1"    b="2"  "#);
        assert_eq!(
            parsed.get("a"),
            Some(&Argument::Literal("1".to_string(), vec![]))
        );
        assert_eq!(
            parsed.get("b"),
            Some(&Argument::Literal("2".to_string(), vec![]))
        );
    }

    #[test]
    fn test_parse_args_literal_with_empty_value() {
        let parsed = parse_args(r#"name="""#);
        assert_eq!(
            parsed.get("name"),
            Some(&Argument::Literal(String::new(), vec![]))
        );
    }

    #[test]
    fn test_parse_args_bareword_between_pairs_is_shorthand() {
        let parsed = parse_args(r#"a="1" justatoken b="2""#);
        // A bareword token with no `=` is shorthand for `name=name`, and
        // parsing continues with any pairs that follow it.
        assert_eq!(
            parsed.get("a"),
            Some(&Argument::Literal("1".to_string(), vec![]))
        );
        assert_eq!(
            parsed.get("justatoken"),
            Some(&Argument::Variable("justatoken", vec![]))
        );
        assert_eq!(
            parsed.get("b"),
            Some(&Argument::Literal("2".to_string(), vec![]))
        );
    }

    #[test]
    fn test_parse_args_single_bareword_is_shorthand() {
        let parsed = parse_args("var_name");
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed.get("var_name"),
            Some(&Argument::Variable("var_name", vec![]))
        );
    }

    #[test]
    fn test_parse_args_mixed_named_and_shorthand() {
        let parsed = parse_args("count=my_count active");
        assert_eq!(
            parsed.get("count"),
            Some(&Argument::Variable("my_count", vec![]))
        );
        assert_eq!(
            parsed.get("active"),
            Some(&Argument::Variable("active", vec![]))
        );
    }

    #[test]
    fn test_parse_args_unterminated_quote_still_captures_partial_value() {
        let parsed = parse_args(r#"name="unterminated"#);
        assert_eq!(
            parsed.get("name"),
            Some(&Argument::Literal("unterminated".to_string(), vec![]))
        );
    }

    #[test]
    fn test_parse_args_variable_with_filter_no_space() {
        // `{{.include x.md greete|uppercase}}` — bareword shorthand whose
        // variable name is immediately followed by a filter, no whitespace.
        let parsed = parse_args("greeter|uppercase");
        assert_eq!(
            parsed.get("greeter"),
            Some(&Argument::Variable("greeter", vec!["uppercase"]))
        );
    }

    #[test]
    fn test_parse_args_key_value_variable_with_filter_after_space() {
        // `{{.include x.md greeter=greeter |uppercase}}` — explicit
        // key=value pair, with the filter separated from the value by
        // whitespace rather than attached directly.
        let parsed = parse_args("greeter=greeter |uppercase");
        assert_eq!(
            parsed.get("greeter"),
            Some(&Argument::Variable("greeter", vec!["uppercase"]))
        );
    }

    #[test]
    fn test_parse_args_variable_with_chained_filters() {
        let parsed = parse_args("name=my_name | upper | trim");
        assert_eq!(
            parsed.get("name"),
            Some(&Argument::Variable("my_name", vec!["upper", "trim"]))
        );
    }

    #[test]
    fn test_parse_args_literal_with_filter() {
        let parsed = parse_args(r#"name="bob" | upper"#);
        assert_eq!(
            parsed.get("name"),
            Some(&Argument::Literal("bob".to_string(), vec!["upper"]))
        );
    }

    #[test]
    fn test_parse_args_filtered_arg_followed_by_another_pair() {
        let parsed = parse_args(r#"a=my_var|upper b="2""#);
        assert_eq!(
            parsed.get("a"),
            Some(&Argument::Variable("my_var", vec!["upper"]))
        );
        assert_eq!(
            parsed.get("b"),
            Some(&Argument::Literal("2".to_string(), vec![]))
        );
    }
}
