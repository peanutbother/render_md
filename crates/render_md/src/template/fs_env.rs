use super::args::{Argument, parse_args};
use super::environment::Environment;
use super::evaluator::Evaluator;
use super::filters;
use crate::{Error, RenderOptions};
use miette::{NamedSource, SourceSpan};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

/// Filesystem-backed environment for template evaluation.
pub struct FileSystemEnvironment<'a, T: gray_matter::engine::Engine> {
    pub src_dir: &'a Path,
    pub current_file: PathBuf,
    pub vars: HashMap<String, String>,
    pub options: &'a RenderOptions,
    pub depth: usize,
    _matter: PhantomData<T>,
}

impl<'a, T: gray_matter::engine::Engine> FileSystemEnvironment<'a, T> {
    pub fn new(
        src_dir: &'a Path,
        current_file: PathBuf,
        vars: HashMap<String, String>,
        options: &'a RenderOptions,
        depth: usize,
    ) -> Self {
        Self {
            src_dir,
            current_file,
            vars,
            options,
            depth,
            _matter: PhantomData,
        }
    }

    fn child(&self, child_file: PathBuf, new_vars: HashMap<String, String>) -> Self {
        // Note: no recursion-depth check here. `resolve_file` (this
        // struct's only caller of `child`) already checks
        // `self.depth >= self.options.max_include_depth` before ever
        // reaching this point, so by the time `child` runs the depth is
        // guaranteed to be within bounds.
        Self::new(
            self.src_dir,
            child_file,
            new_vars,
            self.options,
            self.depth + 1,
        )
    }

    /// Joins `file` against the current file's parent, normalizes any
    /// `..`/`.` components, and validates the result is both inside
    /// `src_dir` and exists on disk.
    fn resolve_include_path(
        &self,
        file: &str,
        span: SourceSpan,
        named_src: &NamedSource<String>,
    ) -> Result<PathBuf, Error> {
        let parent_dir = self.current_file.parent().unwrap_or(self.src_dir);
        let include_path = parent_dir.join(file);
        let clean = clean_path(&include_path);

        if !clean.starts_with(self.src_dir) {
            return Err(Error::include_path_traversal(
                file.to_string(),
                named_src.clone(),
                span,
            ));
        }

        if !clean.exists() {
            return Err(Error::include_file_not_found(
                file.to_string(),
                named_src.clone(),
                span,
            ));
        }

        Ok(clean)
    }

    /// Parses an included file's frontmatter (if any) into its content body
    /// plus its declared `args` schema, split into defaults vs. required
    /// names. Pure w.r.t. `self`; only depends on the generic engine `T`.
    fn parse_component_schema(raw_content: &str) -> (String, Vec<String>, HashMap<String, String>) {
        #[derive(serde::Deserialize, Default)]
        struct ComponentMatter {
            args: Option<HashMap<String, Option<String>>>,
        }

        let matter = gray_matter::Matter::<T>::new();

        let mut required_args = Vec::new();
        let mut default_args = HashMap::new();
        let mut content = raw_content.to_string();

        // Parse frontmatter. If parsing fails, we assume no frontmatter is present.
        if let Ok(result) = matter.parse::<ComponentMatter>(raw_content) {
            content = result.content;
            if let Some(data) = result.data
                && let Some(args_schema) = data.args
            {
                for (k, v) in args_schema {
                    if let Some(val) = v {
                        default_args.insert(k, val);
                    } else {
                        required_args.push(k);
                    }
                }
            }
        }

        (content, required_args, default_args)
    }

    /// Merges default args, then caller-passed args (literal or resolved
    /// from the parent scope's vars), validating all required args are
    /// present.
    fn resolve_include_args(
        &self,
        args_str: &str,
        required_args: Vec<String>,
        default_args: HashMap<String, String>,
        file: &str,
        span: SourceSpan,
        named_src: &NamedSource<String>,
    ) -> Result<HashMap<String, String>, Error> {
        let tag_args = parse_args(args_str);
        let mut resolved_args = default_args;

        for (k, arg) in tag_args {
            let (mut val, arg_filters) = match arg {
                Argument::Literal(val, filters) => {
                    // Interpolate any `{{.var ...}}`/`{{.include ...}}` tags
                    // embedded in the literal before using it.
                    (self.resolve_literal(&val)?, filters)
                }
                Argument::Variable(var_name, filters) => {
                    // Resolve variable from parent scope
                    (self.resolve_var(var_name, span, named_src)?, filters)
                }
            };

            for filter_name in arg_filters {
                val = filters::apply_filter(val, filter_name, span, named_src)?;
            }

            resolved_args.insert(k.to_string(), val);
        }

        for req in required_args {
            if !resolved_args.contains_key(&req) {
                return Err(Error::missing_include_argument(
                    file.to_string(),
                    req,
                    named_src.clone(),
                    span,
                ));
            }
        }

        Ok(resolved_args)
    }

    /// HTML includes get their leading whitespace trimmed and are padded
    /// with blank lines so pulldown-cmark treats them as a standalone block.
    fn normalize_html_content(content: String) -> String {
        let trimmed = content
            .lines()
            .map(|line| line.trim_start())
            .collect::<Vec<&str>>()
            .join("\n");
        format!("\n\n{}\n\n", trimmed)
    }
}

impl<'a, T: gray_matter::engine::Engine> Environment for FileSystemEnvironment<'a, T> {
    fn resolve_var(
        &self,
        var_name: &str,
        span: SourceSpan,
        named_src: &NamedSource<String>,
    ) -> Result<String, Error> {
        self.vars
            .get(var_name)
            .cloned()
            .ok_or_else(|| Error::variable_not_found(var_name.to_string(), named_src.clone(), span))
    }

    fn resolve_file(
        &self,
        file: &str,
        args_str: &str,
        span: SourceSpan,
        named_src: &NamedSource<String>,
    ) -> Result<String, Error> {
        let clean_path = self.resolve_include_path(file, span, named_src)?;

        if self.depth >= self.options.max_include_depth {
            return Err(Error::max_recursion_depth_exceeded(
                named_src.clone(),
                span,
                self.depth,
                self.options.max_include_depth,
            ));
        }

        let raw_content = std::fs::read_to_string(&clean_path)?;
        let (mut included_content, required_args, default_args) =
            Self::parse_component_schema(&raw_content);

        let resolved_args = self.resolve_include_args(
            args_str,
            required_args,
            default_args,
            file,
            span,
            named_src,
        )?;

        let mut child_vars = self.vars.clone();
        child_vars.extend(resolved_args);

        if clean_path.extension().is_some_and(|ext| ext == "html") {
            included_content = Self::normalize_html_content(included_content);
        }

        let child_env = self.child(clean_path.clone(), child_vars);

        Evaluator::evaluate(&included_content, &clean_path, &child_env)
    }

    fn resolve_block(&self, block_name_or_classes: &str) -> String {
        let classes = self
            .options
            .blocks
            .get(block_name_or_classes)
            .map(|s| s.as_str())
            .unwrap_or(block_name_or_classes);

        format!("\n\n<div class=\"{}\">\n\n", classes)
    }

    fn resolve_end(&self) -> String {
        "\n\n</div>\n\n".to_string()
    }

    fn lookup_var(&self, var_name: &str) -> Option<String> {
        self.vars.get(var_name).cloned()
    }

    fn resolve_literal(&self, raw: &str) -> Result<String, Error> {
        // Bump depth by one (via the same `child()` helper `resolve_file`
        // uses) so templates reachable only through interpolated literals
        // (not through a plain include's body content) are still bounded by
        // `max_include_depth`. Without this, two files that reference each
        // other only via include-arg literals could recurse unboundedly:
        // each hop would reuse `self` at a constant depth, never tripping
        // the depth check in `resolve_file`.
        let interp_env = self.child(self.current_file.clone(), self.vars.clone());
        Evaluator::evaluate(raw, &self.current_file, &interp_env)
    }
}

// A simple utility to normalize paths containing ".."
pub(crate) fn clean_path(path: &Path) -> std::path::PathBuf {
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            _ => normalized.push(component),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    type T = gray_matter::engine::YAML;

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src/pages");
        fs::create_dir_all(&src_dir).unwrap();
        (tmp, src_dir)
    }

    fn span() -> SourceSpan {
        SourceSpan::new(0.into(), 0)
    }

    fn named_src() -> NamedSource<String> {
        NamedSource::new("test.md", String::new())
    }

    #[test]
    fn test_resolve_file_rejects_path_traversal() {
        let (_tmp, src_dir) = setup();
        let options = RenderOptions::default();
        let env = FileSystemEnvironment::<T>::new(
            &src_dir,
            src_dir.join("index.md"),
            HashMap::new(),
            &options,
            0,
        );

        let err = env
            .resolve_file("../secret.txt", "", span(), &named_src())
            .unwrap_err();
        match err.inner() {
            crate::ErrorKind::IncludePathTraversal { include_path, .. } => {
                assert_eq!(include_path, "../secret.txt");
            }
            _ => panic!("expected IncludePathTraversal, got {:?}", err),
        }
    }

    #[test]
    fn test_resolve_file_recursion_depth_limit() {
        let (_tmp, src_dir) = setup();
        fs::write(src_dir.join("partial.md"), "hi").unwrap();
        let options = RenderOptions {
            max_include_depth: 1,
            ..Default::default()
        };
        // depth == max_include_depth: the next include would exceed the limit.
        let env = FileSystemEnvironment::<T>::new(
            &src_dir,
            src_dir.join("index.md"),
            HashMap::new(),
            &options,
            1,
        );

        let err = env
            .resolve_file("partial.md", "", span(), &named_src())
            .unwrap_err();
        match err.inner() {
            crate::ErrorKind::MaxRecursionDepthExceeded {
                depth, max_depth, ..
            } => {
                assert_eq!(*depth, 1);
                assert_eq!(*max_depth, 1);
            }
            _ => panic!("expected MaxRecursionDepthExceeded, got {:?}", err),
        }
    }

    #[test]
    fn test_resolve_file_default_arg_used_when_missing() {
        let (_tmp, src_dir) = setup();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    name: \"stranger\"\n---\nHi {{.var name}}",
        )
        .unwrap();
        let options = RenderOptions::default();
        let env = FileSystemEnvironment::<T>::new(
            &src_dir,
            src_dir.join("index.md"),
            HashMap::new(),
            &options,
            0,
        );

        let result = env
            .resolve_file("greeting.md", "", span(), &named_src())
            .unwrap();
        assert_eq!(result, "Hi stranger");
    }

    #[test]
    fn test_resolve_file_literal_arg_overrides_default() {
        let (_tmp, src_dir) = setup();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    name: \"stranger\"\n---\nHi {{.var name}}",
        )
        .unwrap();
        let options = RenderOptions::default();
        let env = FileSystemEnvironment::<T>::new(
            &src_dir,
            src_dir.join("index.md"),
            HashMap::new(),
            &options,
            0,
        );

        let result = env
            .resolve_file("greeting.md", r#"name="Bob""#, span(), &named_src())
            .unwrap();
        assert_eq!(result, "Hi Bob");
    }

    #[test]
    fn test_resolve_file_missing_required_arg_returns_typed_error() {
        let (_tmp, src_dir) = setup();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    name:\n---\nHi {{.var name}}",
        )
        .unwrap();
        let options = RenderOptions::default();
        let env = FileSystemEnvironment::<T>::new(
            &src_dir,
            src_dir.join("index.md"),
            HashMap::new(),
            &options,
            0,
        );

        let err = env
            .resolve_file("greeting.md", "", span(), &named_src())
            .unwrap_err();
        match err.inner() {
            crate::ErrorKind::MissingIncludeArgument {
                include_path,
                arg_name,
                ..
            } => {
                assert_eq!(include_path, "greeting.md");
                assert_eq!(arg_name, "name");
            }
            _ => panic!("expected MissingIncludeArgument, got {:?}", err),
        }
    }

    #[test]
    fn test_resolve_file_normalizes_html_whitespace() {
        let (_tmp, src_dir) = setup();
        fs::write(
            src_dir.join("partial.html"),
            "  <div>\n    <p>hi</p>\n  </div>",
        )
        .unwrap();
        let options = RenderOptions::default();
        let env = FileSystemEnvironment::<T>::new(
            &src_dir,
            src_dir.join("index.md"),
            HashMap::new(),
            &options,
            0,
        );

        let result = env
            .resolve_file("partial.html", "", span(), &named_src())
            .unwrap();
        assert_eq!(result, "\n\n<div>\n<p>hi</p>\n</div>\n\n");
    }

    #[test]
    fn test_lookup_var_returns_none_for_missing() {
        let (_tmp, src_dir) = setup();
        let options = RenderOptions::default();
        let env = FileSystemEnvironment::<T>::new(
            &src_dir,
            src_dir.join("index.md"),
            HashMap::new(),
            &options,
            0,
        );
        assert_eq!(env.lookup_var("missing"), None);
    }

    #[test]
    fn test_lookup_var_returns_some_for_present() {
        let (_tmp, src_dir) = setup();
        let options = RenderOptions::default();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        let env = FileSystemEnvironment::<T>::new(&src_dir, src_dir.join("index.md"), vars, &options, 0);
        assert_eq!(env.lookup_var("name"), Some("Alice".to_string()));
    }

    #[test]
    fn test_resolve_literal_interpolates_var_and_include() {
        let (_tmp, src_dir) = setup();
        fs::write(src_dir.join("partial.md"), "included").unwrap();
        let options = RenderOptions::default();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        let env = FileSystemEnvironment::<T>::new(&src_dir, src_dir.join("index.md"), vars, &options, 0);

        let result = env
            .resolve_literal("Hi {{.var name}}, {{.include partial.md}}")
            .unwrap();
        assert_eq!(result, "Hi Alice, included");
    }

    #[test]
    fn test_resolve_file_literal_arg_interpolates_var_tag() {
        let (_tmp, src_dir) = setup();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    msg:\n---\n{{.var msg}}",
        )
        .unwrap();
        let options = RenderOptions::default();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        let env = FileSystemEnvironment::<T>::new(&src_dir, src_dir.join("index.md"), vars, &options, 0);

        let result = env
            .resolve_file(
                "greeting.md",
                r#"msg="Hello {{.var name}}""#,
                span(),
                &named_src(),
            )
            .unwrap();
        assert_eq!(result, "Hello Alice");
    }

    #[test]
    fn test_resolve_file_variable_arg_shorthand_with_filter_applies_it() {
        // `{{.include greeter.md greeter|uppercase}}` — bareword shorthand
        // (no explicit `key=`) immediately followed by a filter, no
        // whitespace before the `|`.
        let (_tmp, src_dir) = setup();
        fs::write(
            src_dir.join("greeter.md"),
            "---\nargs:\n    greeter:\n---\n{{.var greeter}}",
        )
        .unwrap();
        let options = RenderOptions::default();
        let mut vars = HashMap::new();
        vars.insert("greeter".to_string(), "world".to_string());
        let env = FileSystemEnvironment::<T>::new(&src_dir, src_dir.join("index.md"), vars, &options, 0);

        let result = env
            .resolve_file("greeter.md", "greeter|upper", span(), &named_src())
            .unwrap();
        assert_eq!(result, "WORLD");
    }

    #[test]
    fn test_resolve_file_key_value_arg_with_filter_after_space_applies_it() {
        // `{{.include greeter.md greeter=greeter |uppercase}}` — explicit
        // key=value pair, with the filter separated from the value by
        // whitespace.
        let (_tmp, src_dir) = setup();
        fs::write(
            src_dir.join("greeter.md"),
            "---\nargs:\n    greeter:\n---\n{{.var greeter}}",
        )
        .unwrap();
        let options = RenderOptions::default();
        let mut vars = HashMap::new();
        vars.insert("greeter".to_string(), "world".to_string());
        let env = FileSystemEnvironment::<T>::new(&src_dir, src_dir.join("index.md"), vars, &options, 0);

        let result = env
            .resolve_file("greeter.md", "greeter=greeter |upper", span(), &named_src())
            .unwrap();
        assert_eq!(result, "WORLD");
    }

    #[test]
    fn test_resolve_file_literal_arg_interpolation_respects_max_recursion_depth() {
        // `loop.md` includes itself, passing an include-arg literal that in
        // turn includes `loop.md` again — recursion reachable only through
        // interpolated literals, never through plain body content. This
        // must still hit `MaxRecursionDepthExceeded` (bounded, terminating)
        // rather than recursing unboundedly.
        let (_tmp, src_dir) = setup();
        fs::write(
            src_dir.join("loop.md"),
            r#"{{.include loop.md payload="{{.include loop.md}}"}}"#,
        )
        .unwrap();
        let options = RenderOptions {
            max_include_depth: 3,
            ..Default::default()
        };
        let env = FileSystemEnvironment::<T>::new(
            &src_dir,
            src_dir.join("index.md"),
            HashMap::new(),
            &options,
            0,
        );

        let err = env
            .resolve_file("loop.md", "", span(), &named_src())
            .unwrap_err();
        assert!(matches!(
            err.inner(),
            crate::ErrorKind::MaxRecursionDepthExceeded { .. }
        ));
    }
}
