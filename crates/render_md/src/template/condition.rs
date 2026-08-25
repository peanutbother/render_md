use super::args::parse_quoted_literal;
use super::environment::Environment;
use crate::Error;
use std::fmt;

/// One side of a condition: a variable reference or a quoted literal.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Operand<'a> {
    Var(&'a str),
    Literal(String),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CompareOp {
    Eq,
    Ne,
}

/// A parsed `.if` condition. Grammar (whitespace-tolerant, `==`/`!=` require
/// surrounding whitespace):
///   condition := ['!'] operand
///              | operand ('==' | '!=') operand
///   operand   := '"' ... '"' | bareword
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Condition<'a> {
    Truthy { operand: Operand<'a>, negate: bool },
    Compare {
        lhs: Operand<'a>,
        op: CompareOp,
        rhs: Operand<'a>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConditionParseError {
    UnterminatedQuote,
    EmptyOperand,
    MissingOperator,
    MissingOperand,
    TrailingGarbage(String),
}

impl fmt::Display for ConditionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConditionParseError::UnterminatedQuote => write!(f, "unterminated quoted string"),
            ConditionParseError::EmptyOperand => write!(f, "expected a variable name or a quoted string"),
            ConditionParseError::MissingOperator => {
                write!(f, "expected '==' or '!=' after the first operand")
            }
            ConditionParseError::MissingOperand => {
                write!(f, "expected an operand after the comparison operator")
            }
            ConditionParseError::TrailingGarbage(rest) => {
                write!(f, "unexpected trailing text: '{rest}'")
            }
        }
    }
}

/// Parses a `.if` directive's raw condition text (see [`Condition`] for the
/// grammar). Parsing is deferred to evaluation time (mirroring how
/// `Directive::Include`'s `args` string is parsed lazily by `fs_env.rs`
/// rather than at lex time) so that a condition inside an already-suppressed
/// branch is never parsed at all — see `Evaluator::evaluate`.
pub fn parse_condition(s: &str) -> Result<Condition<'_>, ConditionParseError> {
    let s = s.trim();

    if let Some(rest) = s.strip_prefix('!') {
        let (operand, rest) = parse_operand(rest.trim_start())?;
        let rest = rest.trim();
        if !rest.is_empty() {
            return Err(ConditionParseError::TrailingGarbage(rest.to_string()));
        }
        return Ok(Condition::Truthy {
            operand,
            negate: true,
        });
    }

    let (lhs, rest) = parse_operand(s)?;
    let rest = rest.trim_start();
    if rest.is_empty() {
        return Ok(Condition::Truthy {
            operand: lhs,
            negate: false,
        });
    }

    let (op, rest) = if let Some(r) = rest.strip_prefix("==") {
        (CompareOp::Eq, r)
    } else if let Some(r) = rest.strip_prefix("!=") {
        (CompareOp::Ne, r)
    } else {
        return Err(ConditionParseError::MissingOperator);
    };

    let rest = rest.trim_start();
    if rest.is_empty() {
        return Err(ConditionParseError::MissingOperand);
    }
    let (rhs, rest) = parse_operand(rest)?;
    let rest = rest.trim();
    if !rest.is_empty() {
        return Err(ConditionParseError::TrailingGarbage(rest.to_string()));
    }

    Ok(Condition::Compare { lhs, op, rhs })
}

fn parse_operand(s: &str) -> Result<(Operand<'_>, &str), ConditionParseError> {
    if s.starts_with('"') {
        let (value, rest, closed) = parse_quoted_literal(s);
        if !closed {
            return Err(ConditionParseError::UnterminatedQuote);
        }
        Ok((Operand::Literal(value), rest))
    } else {
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        let ident = &s[..end];
        if ident.is_empty() {
            Err(ConditionParseError::EmptyOperand)
        } else {
            Ok((Operand::Var(ident), s[end..].trim_start()))
        }
    }
}

impl<'a> Operand<'a> {
    /// Resolves this operand to its string value. A missing variable
    /// resolves to `None` (falsy for truthiness, empty-string for
    /// comparisons) rather than erroring — unlike `{{.var}}`, `.if`
    /// conditions never error on a missing/absent variable.
    fn resolve(&self, env: &impl Environment) -> Result<Option<String>, Error> {
        match self {
            Operand::Var(name) => Ok(env.lookup_var(name)),
            Operand::Literal(raw) => Ok(Some(env.resolve_literal(raw)?)),
        }
    }
}

impl<'a> Condition<'a> {
    /// Evaluates this condition to a boolean, resolving any variable/literal
    /// operands (including interpolating any nested tags in a literal
    /// operand) against `env`.
    pub fn evaluate(&self, env: &impl Environment) -> Result<bool, Error> {
        match self {
            Condition::Truthy { operand, negate } => {
                let truthy = operand.resolve(env)?.is_some_and(|v| !v.is_empty());
                Ok(truthy ^ negate)
            }
            Condition::Compare { lhs, op, rhs } => {
                let l = lhs.resolve(env)?.unwrap_or_default();
                let r = rhs.resolve(env)?.unwrap_or_default();
                Ok(match op {
                    CompareOp::Eq => l == r,
                    CompareOp::Ne => l != r,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_condition_truthy_var() {
        let cond = parse_condition("flag").unwrap();
        assert_eq!(
            cond,
            Condition::Truthy {
                operand: Operand::Var("flag"),
                negate: false,
            }
        );
    }

    #[test]
    fn test_parse_condition_negated_var() {
        let cond = parse_condition("!flag").unwrap();
        assert_eq!(
            cond,
            Condition::Truthy {
                operand: Operand::Var("flag"),
                negate: true,
            }
        );
    }

    #[test]
    fn test_parse_condition_negated_var_tolerates_space() {
        let cond = parse_condition("! flag").unwrap();
        assert_eq!(
            cond,
            Condition::Truthy {
                operand: Operand::Var("flag"),
                negate: true,
            }
        );
    }

    #[test]
    fn test_parse_condition_eq_literal() {
        let cond = parse_condition(r#"name == "Bob""#).unwrap();
        assert_eq!(
            cond,
            Condition::Compare {
                lhs: Operand::Var("name"),
                op: CompareOp::Eq,
                rhs: Operand::Literal("Bob".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_condition_ne_literal() {
        let cond = parse_condition(r#"name != "Bob""#).unwrap();
        assert_eq!(
            cond,
            Condition::Compare {
                lhs: Operand::Var("name"),
                op: CompareOp::Ne,
                rhs: Operand::Literal("Bob".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_condition_eq_var_var() {
        let cond = parse_condition("a == b").unwrap();
        assert_eq!(
            cond,
            Condition::Compare {
                lhs: Operand::Var("a"),
                op: CompareOp::Eq,
                rhs: Operand::Var("b"),
            }
        );
    }

    #[test]
    fn test_parse_condition_literal_lhs() {
        let cond = parse_condition(r#""value" == var"#).unwrap();
        assert_eq!(
            cond,
            Condition::Compare {
                lhs: Operand::Literal("value".to_string()),
                op: CompareOp::Eq,
                rhs: Operand::Var("var"),
            }
        );
    }

    #[test]
    fn test_parse_condition_unterminated_quote_errors() {
        let err = parse_condition(r#"name == "unterminated"#).unwrap_err();
        assert_eq!(err, ConditionParseError::UnterminatedQuote);
    }

    #[test]
    fn test_parse_condition_missing_operator_errors() {
        let err = parse_condition("var extra").unwrap_err();
        assert_eq!(err, ConditionParseError::MissingOperator);
    }

    #[test]
    fn test_parse_condition_trailing_garbage_after_compare_errors() {
        let err = parse_condition(r#"var == "x" extra"#).unwrap_err();
        assert_eq!(
            err,
            ConditionParseError::TrailingGarbage("extra".to_string())
        );
    }

    #[test]
    fn test_parse_condition_empty_operand_after_operator_errors() {
        let err = parse_condition("var == ").unwrap_err();
        assert_eq!(err, ConditionParseError::MissingOperand);
    }

    #[test]
    fn test_parse_condition_tolerates_extra_whitespace() {
        let cond = parse_condition(r#"  name   ==   "Bob"  "#).unwrap();
        assert_eq!(
            cond,
            Condition::Compare {
                lhs: Operand::Var("name"),
                op: CompareOp::Eq,
                rhs: Operand::Literal("Bob".to_string()),
            }
        );
    }
}
