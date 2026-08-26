use super::condition;
use super::environment::Environment;
use super::filters;
use super::lexer::{Directive, TagScanner, unescape_tag_markers};
use crate::Error;
use miette::SourceSpan;
use std::path::Path;

pub struct Evaluator;

/// A currently-open `.block`/`.end` or `.if`/`[.else]`/`.end` construct,
/// tracked so a single shared `{{.end}}` keyword can correctly close either
/// kind. `Block` frames never gate emission (they only carry a span for
/// `UnclosedBlock` error reporting); `If` frames do.
enum Frame {
    Block(SourceSpan),
    If {
        span: SourceSpan,
        /// This `.if`'s own condition result (meaningless while
        /// `was_emitting` is false — never even computed in that case).
        cond_true: bool,
        in_else: bool,
        /// Emission state right before this `.if` was opened; restored
        /// (possibly flipped for `.else`) on `.end`/`.else`.
        was_emitting: bool,
    },
}

impl Evaluator {
    /// Evaluates a template string using the provided Environment.
    pub fn evaluate(
        content: &str,
        file_path: &Path,
        env: &impl Environment,
    ) -> Result<String, Error> {
        let scanner = TagScanner::new(content, file_path);
        let mut result = String::new();
        let mut cursor = 0;
        let mut frame_stack: Vec<Frame> = Vec::new();
        let mut is_emitting = true;

        while let Some(tag_match) = scanner.next_tag(cursor)? {
            if is_emitting {
                result.push_str(&unescape_tag_markers(
                    &content[cursor..tag_match.match_start],
                ));
            }

            let resolved_value = match tag_match.directive {
                Directive::Var(spec) => {
                    if is_emitting {
                        let (var_name, filter_names) = filters::parse_var_spec(spec);
                        let mut value =
                            env.resolve_var(var_name, tag_match.span, &scanner.named_src)?;
                        for filter_name in filter_names {
                            value = filters::apply_filter(
                                value,
                                filter_name,
                                tag_match.span,
                                &scanner.named_src,
                            )?;
                        }
                        value
                    } else {
                        String::new()
                    }
                }
                Directive::Include { file, args } => {
                    if is_emitting {
                        env.resolve_file(file, args, tag_match.span, &scanner.named_src)?
                    } else {
                        String::new()
                    }
                }
                Directive::Block(name) => {
                    frame_stack.push(Frame::Block(tag_match.span));
                    if is_emitting {
                        env.resolve_block(name)
                    } else {
                        String::new()
                    }
                }
                Directive::If(cond_str) => {
                    let cond_true = if is_emitting {
                        let parsed = condition::parse_condition(cond_str).map_err(|e| {
                            Error::invalid_condition(
                                e.to_string(),
                                scanner.named_src.clone(),
                                tag_match.span,
                            )
                        })?;
                        parsed.evaluate(env)?
                    } else {
                        // Parent already suppressed: don't parse or
                        // evaluate this condition at all, so a malformed
                        // or var-referencing condition in dead code never
                        // errors.
                        false
                    };
                    frame_stack.push(Frame::If {
                        span: tag_match.span,
                        cond_true,
                        in_else: false,
                        was_emitting: is_emitting,
                    });
                    is_emitting = is_emitting && cond_true;
                    String::new()
                }
                Directive::Else => {
                    match frame_stack.last_mut() {
                        Some(Frame::If {
                            cond_true,
                            in_else,
                            was_emitting,
                            ..
                        }) if !*in_else => {
                            *in_else = true;
                            is_emitting = *was_emitting && !*cond_true;
                        }
                        // A second `.else` for the same `.if`, or a stray
                        // `.else` with no enclosing `.if` at all: tolerated
                        // as a no-op, same precedent as stray `.end`.
                        _ => {}
                    }
                    String::new()
                }
                Directive::End => match frame_stack.pop() {
                    Some(Frame::If { was_emitting, .. }) => {
                        is_emitting = was_emitting;
                        // Closing an `.if`/`.else` never emits `</div>`.
                        String::new()
                    }
                    Some(Frame::Block(_)) => {
                        if is_emitting {
                            env.resolve_end()
                        } else {
                            String::new()
                        }
                    }
                    None => {
                        // Stray `.end`, nothing open — tolerated (existing
                        // precedent: `test_evaluate_stray_end_tag_is_tolerated`).
                        if is_emitting {
                            env.resolve_end()
                        } else {
                            String::new()
                        }
                    }
                },
            };

            result.push_str(&resolved_value);
            cursor = tag_match.match_end;
        }

        if let Some(frame) = frame_stack.pop() {
            return Err(match frame {
                Frame::Block(span) => Error::unclosed_block(scanner.named_src.clone(), span),
                Frame::If { span, .. } => Error::unclosed_if(scanner.named_src.clone(), span),
            });
        }

        if is_emitting {
            result.push_str(&unescape_tag_markers(&content[cursor..]));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miette::{NamedSource, SourceSpan};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// A stub `Environment` for testing the evaluator in isolation, without
    /// needing a real filesystem-backed implementation.
    struct FakeEnv {
        vars: HashMap<String, String>,
        included: RefCell<Vec<String>>,
    }

    impl FakeEnv {
        fn new() -> Self {
            Self {
                vars: HashMap::new(),
                included: RefCell::new(Vec::new()),
            }
        }

        fn with_var(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_string(), value.to_string());
            self
        }
    }

    impl Environment for FakeEnv {
        fn resolve_var(
            &self,
            var_name: &str,
            span: SourceSpan,
            named_src: &NamedSource<String>,
        ) -> Result<String, Error> {
            self.vars.get(var_name).cloned().ok_or_else(|| {
                Error::variable_not_found(var_name.to_string(), named_src.clone(), span)
            })
        }

        fn resolve_file(
            &self,
            file: &str,
            args_str: &str,
            _span: SourceSpan,
            _named_src: &miette::NamedSource<String>,
        ) -> Result<String, Error> {
            self.included.borrow_mut().push(file.to_string());
            Ok(format!("[included {file} args={args_str}]"))
        }

        fn resolve_block(&self, block_name_or_classes: &str) -> String {
            format!("<div class=\"{block_name_or_classes}\">")
        }

        fn lookup_var(&self, var_name: &str) -> Option<String> {
            self.vars.get(var_name).cloned()
        }

        fn resolve_literal(&self, raw: &str) -> Result<String, Error> {
            Evaluator::evaluate(raw, &path(), self)
        }

        fn resolve_end(&self) -> String {
            "</div>".to_string()
        }
    }

    fn path() -> PathBuf {
        PathBuf::from("test.md")
    }

    #[test]
    fn test_evaluate_plain_text_is_unchanged() {
        let env = FakeEnv::new();
        let result = Evaluator::evaluate("just plain text", &path(), &env).unwrap();
        assert_eq!(result, "just plain text");
    }

    #[test]
    fn test_evaluate_var_interpolation_preserves_surrounding_text() {
        let env = FakeEnv::new().with_var("name", "World");
        let result = Evaluator::evaluate("Hello {{.var name}}!", &path(), &env).unwrap();
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_evaluate_missing_var_bubbles_up_error() {
        let env = FakeEnv::new();
        let err = Evaluator::evaluate("{{.var missing}}", &path(), &env).unwrap_err();
        match err.inner() {
            crate::ErrorKind::VariableNotFound { var_name, .. } => assert_eq!(var_name, "missing"),
            _ => panic!("expected VariableNotFound"),
        }
    }

    #[test]
    fn test_evaluate_include_delegates_to_environment() {
        let env = FakeEnv::new();
        let result = Evaluator::evaluate(r#"{{.include ./a.md x="1"}}"#, &path(), &env).unwrap();
        assert_eq!(result, r#"[included ./a.md args=x="1"]"#);
        assert_eq!(env.included.borrow().as_slice(), ["./a.md"]);
    }

    #[test]
    fn test_evaluate_block_and_end() {
        let env = FakeEnv::new();
        let result = Evaluator::evaluate("{{.block hero}}content{{.end}}", &path(), &env).unwrap();
        assert_eq!(result, "<div class=\"hero\">content</div>");
    }

    #[test]
    fn test_evaluate_unclosed_block_errors() {
        let env = FakeEnv::new();
        let err = Evaluator::evaluate("{{.block hero}}content", &path(), &env).unwrap_err();
        assert!(matches!(
            err.inner(),
            crate::ErrorKind::UnclosedBlock { .. }
        ));
    }

    #[test]
    fn test_evaluate_stray_end_tag_is_tolerated() {
        // No matching `.block` was opened; the evaluator currently allows this silently.
        let env = FakeEnv::new();
        let result = Evaluator::evaluate("{{.end}}", &path(), &env).unwrap();
        assert_eq!(result, "</div>");
    }

    #[test]
    fn test_evaluate_multiple_tags_in_sequence() {
        let env = FakeEnv::new().with_var("a", "1").with_var("b", "2");
        let result = Evaluator::evaluate("{{.var a}}-{{.var b}}", &path(), &env).unwrap();
        assert_eq!(result, "1-2");
    }

    #[test]
    fn test_evaluate_if_true_renders_body() {
        let env = FakeEnv::new().with_var("flag", "1");
        let result = Evaluator::evaluate("{{.if flag}}shown{{.end}}", &path(), &env).unwrap();
        assert_eq!(result, "shown");
    }

    #[test]
    fn test_evaluate_if_missing_var_is_falsy_no_error() {
        let env = FakeEnv::new();
        let result = Evaluator::evaluate("{{.if missing}}shown{{.end}}", &path(), &env).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_evaluate_if_else_renders_else_branch_when_false() {
        let env = FakeEnv::new();
        let result =
            Evaluator::evaluate("{{.if missing}}A{{.else}}B{{.end}}", &path(), &env).unwrap();
        assert_eq!(result, "B");
    }

    #[test]
    fn test_evaluate_if_negation() {
        let env = FakeEnv::new();
        let result = Evaluator::evaluate("{{.if !missing}}shown{{.end}}", &path(), &env).unwrap();
        assert_eq!(result, "shown");
    }

    #[test]
    fn test_evaluate_if_eq_literal_true() {
        let env = FakeEnv::new().with_var("name", "Bob");
        let result =
            Evaluator::evaluate(r#"{{.if name == "Bob"}}match{{.end}}"#, &path(), &env).unwrap();
        assert_eq!(result, "match");
    }

    #[test]
    fn test_evaluate_if_eq_literal_false() {
        let env = FakeEnv::new().with_var("name", "Alice");
        let result =
            Evaluator::evaluate(r#"{{.if name == "Bob"}}match{{.end}}"#, &path(), &env).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_evaluate_if_ne_var_var() {
        let env = FakeEnv::new().with_var("a", "1").with_var("b", "2");
        let result =
            Evaluator::evaluate("{{.if a != b}}different{{.end}}", &path(), &env).unwrap();
        assert_eq!(result, "different");
    }

    #[test]
    fn test_evaluate_if_end_does_not_emit_closing_div() {
        let env = FakeEnv::new().with_var("flag", "1");
        let result = Evaluator::evaluate("{{.if flag}}content{{.end}}", &path(), &env).unwrap();
        assert_eq!(result, "content");
        assert!(!result.contains("</div>"));
    }

    #[test]
    fn test_evaluate_nested_block_inside_true_if_renders_normally() {
        let env = FakeEnv::new().with_var("flag", "1");
        let result = Evaluator::evaluate(
            "{{.if flag}}{{.block hero}}content{{.end}}{{.end}}",
            &path(),
            &env,
        )
        .unwrap();
        assert_eq!(result, "<div class=\"hero\">content</div>");
    }

    #[test]
    fn test_evaluate_nested_if_inside_false_branch_skips_condition_evaluation_entirely() {
        // If the inner `.if`'s condition were actually parsed/evaluated, its
        // literal operand's embedded `{{.var missing}}` would error via
        // `resolve_literal` -> `resolve_var`. Since the whole thing is
        // inside the outer (false) `.if`'s suppressed region, it must never
        // be touched at all, and evaluation succeeds with empty output.
        let env = FakeEnv::new();
        let template = r#"{{.if outer}}{{.if x == "prefix {{.var missing}}"}}inner{{.end}}shown{{.end}}"#;
        let result = Evaluator::evaluate(template, &path(), &env).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_evaluate_block_inside_false_if_is_suppressed_but_still_tracked_for_matching() {
        let env = FakeEnv::new();
        let template = "{{.if outer}}{{.block hero}}content{{.end}}{{.end}}after";
        let result = Evaluator::evaluate(template, &path(), &env).unwrap();
        assert_eq!(result, "after");
    }

    #[test]
    fn test_evaluate_stray_second_else_is_tolerated_as_noop() {
        let env = FakeEnv::new().with_var("flag", "1");
        let result =
            Evaluator::evaluate("{{.if flag}}A{{.else}}B{{.else}}C{{.end}}", &path(), &env)
                .unwrap();
        assert_eq!(result, "A");
    }

    #[test]
    fn test_evaluate_unclosed_if_errors() {
        let env = FakeEnv::new().with_var("flag", "1");
        let err = Evaluator::evaluate("{{.if flag}}content", &path(), &env).unwrap_err();
        assert!(matches!(err.inner(), crate::ErrorKind::UnclosedIf { .. }));
    }

    #[test]
    fn test_evaluate_var_with_single_filter() {
        let env = FakeEnv::new().with_var("name", "world");
        let result = Evaluator::evaluate("{{.var name | upper}}", &path(), &env).unwrap();
        assert_eq!(result, "WORLD");
    }

    #[test]
    fn test_evaluate_var_with_chained_filters() {
        let env = FakeEnv::new().with_var("name", "  world  ");
        let result = Evaluator::evaluate("{{.var name | trim | upper}}", &path(), &env).unwrap();
        assert_eq!(result, "WORLD");
    }

    #[test]
    fn test_evaluate_var_unknown_filter_errors() {
        let env = FakeEnv::new().with_var("name", "world");
        let err = Evaluator::evaluate("{{.var name | bogus}}", &path(), &env).unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnknownFilter { filter_name, .. } => {
                assert_eq!(filter_name, "bogus")
            }
            _ => panic!("expected UnknownFilter, got {:?}", err),
        }
    }

    #[test]
    fn test_evaluate_var_missing_errors_even_with_filters_chained() {
        let env = FakeEnv::new();
        let err = Evaluator::evaluate("{{.var missing | upper}}", &path(), &env).unwrap_err();
        match err.inner() {
            crate::ErrorKind::VariableNotFound { var_name, .. } => assert_eq!(var_name, "missing"),
            _ => panic!("expected VariableNotFound, got {:?}", err),
        }
    }

    #[test]
    fn test_evaluate_escaped_tag_renders_literally_without_backslash() {
        let env = FakeEnv::new();
        let result = Evaluator::evaluate(r#"\{{.if missing}}"#, &path(), &env).unwrap();
        assert_eq!(result, r#"{{.if missing}}"#);
    }

    #[test]
    fn test_evaluate_escaped_tag_does_not_error_even_if_it_looks_broken() {
        // A directive that would otherwise error (unknown filter, missing
        // var, etc.) is never parsed or evaluated at all once escaped —
        // it's just literal text.
        let env = FakeEnv::new();
        let result = Evaluator::evaluate(r#"\{{.var missing | bogus}}"#, &path(), &env).unwrap();
        assert_eq!(result, r#"{{.var missing | bogus}}"#);
    }

    #[test]
    fn test_evaluate_escaped_tag_mixed_with_real_tag() {
        let env = FakeEnv::new().with_var("name", "World");
        let result = Evaluator::evaluate(
            r#"\{{.var name}} says {{.var name}}"#,
            &path(),
            &env,
        )
        .unwrap();
        assert_eq!(result, r#"{{.var name}} says World"#);
    }

    #[test]
    fn test_evaluate_multiline_escaped_tip_block_renders_literally() {
        // Mirrors a documentation snippet showing the `.if`/`.else`/`.end`
        // syntax itself, escaped so it's not evaluated.
        let env = FakeEnv::new();
        let template = concat!(
            "\\{{.if status == \"online\"}}\n",
            "🟢 Status: **\\{{.var status}}**\n",
            "\\{{.else}}\n",
            "🔴 Status: offline\n",
            "\\{{.end}}",
        );
        let expected = concat!(
            "{{.if status == \"online\"}}\n",
            "🟢 Status: **{{.var status}}**\n",
            "{{.else}}\n",
            "🔴 Status: offline\n",
            "{{.end}}",
        );
        let result = Evaluator::evaluate(template, &path(), &env).unwrap();
        assert_eq!(result, expected);
    }
}
