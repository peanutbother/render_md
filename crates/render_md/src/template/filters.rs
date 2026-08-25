use crate::Error;
use miette::{NamedSource, SourceSpan};

/// Splits a `.var` directive's raw text (e.g. `"name | upper | trim"`) into
/// the variable name and an ordered list of filter names to apply to its
/// resolved value. `{{.var name}}` (no `|`) yields `("name", vec![])`.
pub fn parse_var_spec(spec: &str) -> (&str, Vec<&str>) {
    let mut segments = spec.split('|').map(str::trim);
    let var_name = segments.next().unwrap_or("");
    (var_name, segments.collect())
}

/// Applies one named filter to `value`, returning the transformed string.
/// Unknown filter names produce a spanned `UnknownFilter` error.
pub fn apply_filter(
    value: String,
    filter_name: &str,
    span: SourceSpan,
    named_src: &NamedSource<String>,
) -> Result<String, Error> {
    match filter_name {
        "upper" => Ok(value.to_uppercase()),
        "lower" => Ok(value.to_lowercase()),
        "trim" => Ok(value.trim().to_string()),
        "title" => Ok(title_case(&value)),
        other => Err(Error::unknown_filter(
            other.to_string(),
            named_src.clone(),
            span,
        )),
    }
}

/// Capitalizes the first letter of each whitespace-separated word and
/// lowercases the rest. Collapses internal whitespace runs to single spaces.
fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> SourceSpan {
        SourceSpan::new(0.into(), 0)
    }

    fn named_src() -> NamedSource<String> {
        NamedSource::new("test.md", String::new())
    }

    #[test]
    fn test_parse_var_spec_no_filters() {
        assert_eq!(parse_var_spec("name"), ("name", vec![]));
    }

    #[test]
    fn test_parse_var_spec_single_filter() {
        assert_eq!(parse_var_spec("name | upper"), ("name", vec!["upper"]));
    }

    #[test]
    fn test_parse_var_spec_chained_filters() {
        assert_eq!(
            parse_var_spec("name | upper | trim"),
            ("name", vec!["upper", "trim"])
        );
    }

    #[test]
    fn test_apply_filter_upper() {
        let result = apply_filter("world".to_string(), "upper", span(), &named_src()).unwrap();
        assert_eq!(result, "WORLD");
    }

    #[test]
    fn test_apply_filter_lower() {
        let result = apply_filter("WORLD".to_string(), "lower", span(), &named_src()).unwrap();
        assert_eq!(result, "world");
    }

    #[test]
    fn test_apply_filter_trim() {
        let result = apply_filter("  hi  ".to_string(), "trim", span(), &named_src()).unwrap();
        assert_eq!(result, "hi");
    }

    #[test]
    fn test_apply_filter_title() {
        let result =
            apply_filter("hello world".to_string(), "title", span(), &named_src()).unwrap();
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_apply_filter_unknown_errors() {
        let err = apply_filter("x".to_string(), "bogus", span(), &named_src()).unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnknownFilter { filter_name, .. } => {
                assert_eq!(filter_name, "bogus")
            }
            _ => panic!("expected UnknownFilter, got {:?}", err),
        }
    }
}
