use crate::Error;
use miette::{NamedSource, SourceSpan};

/// Defines the environment/context operations available to the evaluator.
pub trait Environment {
    /// Resolves a variable by name.
    fn resolve_var(
        &self,
        var_name: &str,
        span: SourceSpan,
        named_src: &NamedSource<String>,
    ) -> Result<String, Error>;

    /// Resolves and includes another file.
    fn resolve_file(
        &self,
        file: &str,
        args_str: &str,
        span: SourceSpan,
        named_src: &NamedSource<String>,
    ) -> Result<String, Error>;

    /// Resolves the opening of a block.
    fn resolve_block(&self, block_name_or_classes: &str) -> String;

    /// Resolves the closing of a block.
    fn resolve_end(&self) -> String;

    /// Non-erroring counterpart to `resolve_var`. Used by `.if` condition
    /// evaluation, where a missing/absent variable must be treated as falsy
    /// rather than surfacing a `VariableNotFound` error — unlike
    /// `resolve_var`, which `.var` uses and which always errors on a
    /// missing name. Returns `None` if `var_name` isn't present in the
    /// current scope.
    fn lookup_var(&self, var_name: &str) -> Option<String>;

    /// Interpolates any `{{.var ...}}`/`{{.include ...}}` tags embedded in a
    /// quoted literal string (an include argument's literal, or an `.if`
    /// condition's literal operand) by recursively re-running the evaluator
    /// over `raw`. Implementors are responsible for their own recursion-depth
    /// safety when `raw` can itself contain `.include` tags.
    fn resolve_literal(&self, raw: &str) -> Result<String, Error>;
}
