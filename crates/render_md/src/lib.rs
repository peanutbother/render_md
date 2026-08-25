pub use crate::error::{Error, ErrorKind};
use gray_matter::Matter;
use http::status::StatusCode;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

pub use gray_matter;

mod error;
pub mod markdown;
pub mod styles;
pub mod template;

pub const MAX_INCLUDE_DEPTH_DEFAULT: usize = 10;

/// Engine-level rendering configuration. Constructed programmatically
/// (`main.rs`, tests) — never deserialized from a page's frontmatter, so a
/// page can't accidentally (and silently) override process-wide settings
/// like `detailed_errors`/`max_include_depth` from its own YAML. See
/// [`PageMatter`] for the frontmatter-facing schema.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Used as fallback if no front matter attribute is set
    pub title: Option<String>,
    #[cfg(feature = "detailed-errors")]
    /// Whether to display verbose errors with source mapping or just error message
    pub detailed_errors: bool,
    /// Maximum recursion depth of inclusion directives
    pub max_include_depth: usize,
    /// Mapping of standard HTML tags (e.g., "p", "h1") to CSS classes
    pub tags: HashMap<String, String>,
    /// Custom block mappings: section_name -> CSS classes
    pub blocks: HashMap<String, String>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            title: None,
            #[cfg(feature = "detailed-errors")]
            detailed_errors: false,
            max_include_depth: MAX_INCLUDE_DEPTH_DEFAULT,
            tags: HashMap::new(),
            blocks: HashMap::new(),
        }
    }
}

/// A single page's frontmatter schema. Mirrors the naming convention of
/// [`template::fs_env`]'s `ComponentMatter`.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct PageMatter {
    /// Overrides `RenderOptions::title` for this page, if set.
    pub title: Option<String>,
    /// Mapping of standard HTML tags (e.g., "p", "h1") to CSS classes,
    /// merged onto (and overriding) the engine's configured `tags`.
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// Custom block mappings: section_name -> CSS classes, merged onto
    /// (and overriding) the engine's configured `blocks`.
    #[serde(default)]
    pub blocks: HashMap<String, String>,
    /// Argument schema for this page: name -> default value (`None` means required)
    #[serde(default)]
    pub args: HashMap<String, Option<String>>,
    /// Any remaining keys not matching the above will be stored as custom variables
    #[serde(flatten)]
    pub custom_vars: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RenderPaths {
    pub base_dir: PathBuf,
    pub src_dir: PathBuf,
    pub public_dir: PathBuf,
    pub template_path: PathBuf,
    pub style_path: PathBuf,
}

#[derive(Debug)]
pub struct RenderEngine<T: gray_matter::engine::Engine> {
    pub paths: RenderPaths,
    options: RenderOptions,
    _matter: PhantomData<T>,
}

/// RenderEngine takes T to decide which front matter config style to use
/// `T` needs to implement [`gray_matter::engine::Engine`]
impl<T> RenderEngine<T>
where
    T: gray_matter::engine::Engine,
{
    /// Creates a new `RenderEngine` with the given base path and rendering options.
    ///
    /// The engine will configure the internal paths for source, public, template, and styling
    /// relative to the `base_path` provided.
    pub fn new(render_paths: RenderPaths, options: RenderOptions) -> Self {
        Self {
            paths: render_paths,
            options,
            _matter: PhantomData,
        }
    }

    /// Resolves the filesystem paths for a given URI.
    ///
    /// Given a URI (e.g., `/about`), this determines:
    /// - The source route directory (e.g., `src/pages/about`)
    /// - The source markdown file (e.g., `src/pages/about/index.md`)
    /// - The public HTML output file (e.g., `public/about/index.html`)
    ///
    /// Returns `Some((route_dir, src_md, public_html))` if the route is valid and exists,
    /// otherwise returns `None`.
    pub fn resolve_paths(&self, uri: &str) -> Option<(PathBuf, PathBuf, PathBuf)> {
        let clean_uri = uri.trim_matches('/');
        let clean_uri = clean_uri.trim_end_matches(".md").trim_end_matches(".html");

        let route_dir = if clean_uri.is_empty() {
            self.paths.src_dir.clone()
        } else {
            template::fs_env::clean_path(&self.paths.src_dir.join(clean_uri))
        };

        let src_md = route_dir.join("index.md");

        let public_html_dir = if clean_uri.is_empty() {
            self.paths.public_dir.clone()
        } else {
            self.paths.public_dir.join(clean_uri)
        };
        let public_html = public_html_dir.join("index.html");

        if !route_dir.starts_with(&self.paths.src_dir) || !src_md.exists() {
            return None;
        }

        Some((route_dir, src_md, public_html))
    }

    /// Checks whether the generated HTML output is stale compared to its sources.
    ///
    /// Evaluates the modification times of the template, stylesheet, and all markdown files
    /// within the `route_dir`. Returns `true` if any source file is newer than the `public_html`
    /// or if the `public_html` does not exist.
    pub fn is_stale(&self, route_dir: &Path, public_html: &Path) -> bool {
        let Some(cache_mtime) = Self::mtime_or_log(public_html) else {
            return true;
        };

        let tpl_mtime =
            Self::mtime_or_log(&self.paths.template_path).unwrap_or(SystemTime::UNIX_EPOCH);
        let style_mtime =
            Self::mtime_or_log(&self.paths.style_path).unwrap_or(SystemTime::UNIX_EPOCH);

        let output_css = self.paths.public_dir.join("assets/style.css");
        let cache_style_mtime = Self::mtime_or_log(&output_css).unwrap_or(SystemTime::UNIX_EPOCH);

        if tpl_mtime > cache_mtime || style_mtime > cache_style_mtime {
            return true;
        }

        for entry in WalkDir::new(route_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext == "md" || ext == "html")
            {
                let mtime = Self::mtime_or_log(path).unwrap_or(SystemTime::UNIX_EPOCH);
                if mtime > cache_mtime {
                    return true;
                }
            }
        }

        false
    }

    /// Returns `path`'s modification time, or `None` if it doesn't exist.
    ///
    /// Other IO error kinds (e.g. permission denied) are logged to stderr
    /// rather than being silently conflated with "missing" — callers still
    /// treat them as `None` (safest default: attempt a rebuild rather than
    /// risk serving a stale cached page), but the distinct failure is at
    /// least visible instead of being indistinguishable from a very old file.
    fn mtime_or_log(path: &Path) -> Option<SystemTime> {
        match fs::metadata(path).and_then(|m| m.modified()) {
            Ok(time) => Some(time),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                eprintln!("warning: failed to stat '{}': {e}", path.display());
                None
            }
        }
    }

    /// Reads and parses a page's frontmatter + content.
    fn parse_page(&self, src_md: &Path) -> Result<gray_matter::ParsedEntity<PageMatter>, Error> {
        let raw_content = fs::read_to_string(src_md)?;
        let matter = Matter::<T>::new();
        Ok(matter.parse(&raw_content)?)
    }

    /// Resolves the page's title, merges frontmatter `tags`/`blocks` onto the
    /// engine's configured defaults, and folds `custom_vars` + declared
    /// `args` defaults into `vars` (mutated in place, so callers can inspect
    /// it — e.g. its resolved `title` — after `render_page` returns).
    /// Returns `(title, active_tags, active_blocks)`.
    fn merge_page_vars(
        &self,
        data: Option<&PageMatter>,
        vars: &mut HashMap<String, String>,
    ) -> (String, HashMap<String, String>, HashMap<String, String>) {
        let title = data
            .and_then(|d| d.title.clone())
            .or(self.options.title.clone())
            .unwrap_or_default();

        let mut active_tags = self.options.tags.clone();
        let mut active_blocks = self.options.blocks.clone();

        if let Some(data) = data {
            active_tags.extend(data.tags.clone());
            active_blocks.extend(data.blocks.clone());

            // Add all the custom frontmatter variables into the template engine's `vars`
            for (key, value) in &data.custom_vars {
                vars.insert(key.clone(), value.clone());
            }

            // This page's own `args` schema behaves like `custom_vars` for defaults: any
            // declared default is merged in unless the caller already supplied a value.
            // Required args (no default) aren't validated here — if actually referenced,
            // the evaluator's normal `VariableNotFound` error covers that.
            for (name, default) in &data.args {
                if let Some(default_val) = default {
                    vars.entry(name.clone())
                        .or_insert_with(|| default_val.clone());
                }
            }
        }

        vars.insert("title".to_owned(), title.clone());

        (title, active_tags, active_blocks)
    }

    /// Evaluates a page's body template directives and renders it to HTML.
    fn render_body(
        &self,
        src_md: &Path,
        content: &str,
        vars: HashMap<String, String>,
        active_tags: &HashMap<String, String>,
        active_blocks: HashMap<String, String>,
    ) -> Result<String, Error> {
        let local_options = RenderOptions {
            tags: active_tags.clone(),
            blocks: active_blocks,
            ..self.options.clone()
        };

        let env = template::fs_env::FileSystemEnvironment::<T>::new(
            &self.paths.src_dir,
            src_md.to_path_buf(),
            vars,
            &local_options,
            0,
        );

        let processed_content = template::evaluator::Evaluator::evaluate(content, src_md, &env)?;

        Ok(markdown::render_markdown(&processed_content, active_tags))
    }

    /// Evaluates `template.html`'s directives and substitutes `{{title}}`/`{{body}}`.
    fn apply_template(
        &self,
        vars: HashMap<String, String>,
        title: &str,
        body: &str,
    ) -> Result<String, Error> {
        let template_content = fs::read_to_string(&self.paths.template_path)?;
        let template_env = template::fs_env::FileSystemEnvironment::<T>::new(
            &self.paths.src_dir,
            self.paths.template_path.clone(),
            vars,
            &self.options,
            0,
        );

        let template_processed = template::evaluator::Evaluator::evaluate(
            &template_content,
            &self.paths.template_path,
            &template_env,
        )?;

        Ok(template_processed
            .replace("{{title}}", title)
            .replace("{{body}}", body))
    }

    /// Renders a single markdown page into an HTML string.
    ///
    /// Parses front matter, processes template variables, compiles markdown to HTML,
    /// and substitutes the result into the main HTML template.
    /// Returns the complete HTML string on success, or an `Error` upon failure.
    pub fn render_page(
        &self,
        src_md: &Path,
        vars: &mut HashMap<String, String>,
    ) -> Result<String, Error> {
        let result = self.parse_page(src_md)?;
        let (title, active_tags, active_blocks) = self.merge_page_vars(result.data.as_ref(), vars);
        let html_output = self.render_body(
            src_md,
            &result.content,
            vars.clone(),
            &active_tags,
            active_blocks,
        )?;
        self.apply_template(vars.clone(), &title, &html_output)
    }

    /// Compiles a markdown page and writes the generated HTML to disk.
    ///
    /// Under the hood, this calls `render_page` and saves the output to `public_html`,
    /// creating any necessary parent directories.
    pub fn compile_page(
        &self,
        src_md: &Path,
        public_html: &Path,
        vars: &mut HashMap<String, String>,
    ) -> Result<(), Error> {
        let final_html = self.render_page(src_md, vars)?;

        if let Some(parent) = public_html.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(public_html, final_html)?;
        Ok(())
    }

    /// Prints a basic HTTP error response to standard output.
    ///
    /// Formats the response with the provided HTTP status code and text content.
    /// Useful for CGI-based environments.
    pub fn render_error(&self, status: StatusCode, content: &str) {
        println!(
            "Status: {}\r\nContent-Type: text/html; charset=utf-8\r\n\r\n{}",
            status, content
        );
    }

    /// Formats an error into a string for display.
    ///
    /// If detailed errors are enabled in `RenderOptions`, uses `miette` to format
    /// the error with rich context. Otherwise, uses standard `Display`.
    pub fn format_error(&self, err: Error) -> String {
        #[cfg(feature = "detailed-errors")]
        if self.options.detailed_errors {
            format!("{:?}", miette::Report::new(err.into_inner()))
        } else {
            err.to_string()
        }
        #[cfg(not(feature = "detailed-errors"))]
        err.to_string()
    }

    /// Compiles CSS styles for the project using tailwindcss.
    ///
    /// Uses the executable specified by the `TAILWIND_BIN` environment variable (default: `tailwindcss`)
    /// to compile styles from `style_path` into `public/assets/style.css`.
    /// Returns `Ok(())` on success or an `Error` if the process fails.
    pub fn compile_styles(&self) -> Result<(), Error> {
        styles::compile_styles(
            &self.paths.base_dir,
            &self.paths.style_path,
            &self.paths.public_dir,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    type T = gray_matter::engine::YAML;

    fn setup_env() -> (TempDir, std::path::PathBuf, RenderEngine<T>) {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_dir = temp_dir.path().to_path_buf();
        let src_dir = base_dir.join("src/pages");
        let template_dir = base_dir.join("src");
        let public_dir = base_dir.join("public");
        let template_path = base_dir.join("src").join("template.html");
        let style_dir = base_dir.join("src/styles");
        let style_path = style_dir.join("style.css");

        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&template_dir).unwrap();
        fs::create_dir_all(&public_dir).unwrap();
        fs::create_dir_all(&style_dir).unwrap();
        fs::write(&template_path, "{{body}}").unwrap();
        fs::write(&style_path, "").unwrap();

        let paths = RenderPaths {
            src_dir: src_dir.clone(),
            template_path,
            public_dir,
            style_path,
            base_dir,
        };
        let options = RenderOptions {
            title: None,
            #[cfg(feature = "detailed-errors")]
            detailed_errors: true,
            max_include_depth: 3,
            tags: HashMap::new(),
            blocks: HashMap::new(),
        };

        let engine = RenderEngine::new(paths, options);
        (temp_dir, src_dir, engine)
    }

    /// Writes `content` to `src_dir/filename` and renders it with an empty
    /// vars map. For tests that don't need to seed or inspect `vars`.
    fn render(
        engine: &RenderEngine<T>,
        src_dir: &Path,
        filename: &str,
        content: &str,
    ) -> Result<String, Error> {
        render_with_vars(engine, src_dir, filename, content, &mut HashMap::new())
    }

    /// Writes `content` to `src_dir/filename` and renders it using the
    /// caller's `vars` map, so the caller can pre-populate values before
    /// the call or inspect the map (e.g. frontmatter-derived `title`) after.
    fn render_with_vars(
        engine: &RenderEngine<T>,
        src_dir: &Path,
        filename: &str,
        content: &str,
        vars: &mut HashMap<String, String>,
    ) -> Result<String, Error> {
        let file_path = src_dir.join(filename);
        fs::write(&file_path, content).unwrap();
        engine.render_page(&file_path, vars)
    }

    #[test]
    fn test_variable_interpolation() {
        let (_tmp, src_dir, engine) = setup_env();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "World".to_string());

        let result = render_with_vars(
            &engine,
            &src_dir,
            "test.md",
            "Hello {{.var name}}!",
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>Hello World!</p>\n");
    }

    #[test]
    fn test_page_args_default_applied_when_missing() {
        let (_tmp, src_dir, engine) = setup_env();
        let result = render(
            &engine,
            &src_dir,
            "test.md",
            "---\nargs:\n    error: \"An unknown error occurred\"\n---\n{{.var error}}",
        )
        .unwrap();
        assert_eq!(result, "<p>An unknown error occurred</p>\n");
    }

    #[test]
    fn test_page_args_default_does_not_override_caller_value() {
        let (_tmp, src_dir, engine) = setup_env();
        let mut vars = HashMap::new();
        vars.insert("error".to_string(), "Something specific broke".to_string());

        let result = render_with_vars(
            &engine,
            &src_dir,
            "test.md",
            "---\nargs:\n    error: \"An unknown error occurred\"\n---\n{{.var error}}",
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>Something specific broke</p>\n");
    }

    #[test]
    fn test_page_args_required_without_value_still_errors_naturally() {
        let (_tmp, src_dir, engine) = setup_env();
        let err = render(
            &engine,
            &src_dir,
            "test.md",
            "---\nargs:\n    greeter:\n---\nHello {{.var greeter}}",
        )
        .unwrap_err();
        match err.inner() {
            crate::ErrorKind::VariableNotFound { var_name, .. } => assert_eq!(var_name, "greeter"),
            _ => panic!("Expected VariableNotFound error, got {:?}", err),
        }
    }

    #[test]
    fn test_missing_variable_errors() {
        let (_tmp, src_dir, engine) = setup_env();
        let err = render(&engine, &src_dir, "test.md", "Hello {{.var missing}}!").unwrap_err();

        match err.inner() {
            crate::ErrorKind::VariableNotFound { var_name, .. } => assert_eq!(var_name, "missing"),
            _ => panic!("Expected VariableNotFound error"),
        }
    }

    #[test]
    fn test_file_inclusion() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(src_dir.join("partial.md"), "I am a partial").unwrap();

        let result = render(
            &engine,
            &src_dir,
            "index.md",
            "Include: {{.include partial.md}}",
        )
        .unwrap();
        assert_eq!(result, "<p>Include: I am a partial</p>\n");
    }

    #[test]
    fn test_html_inclusion_newlines() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(src_dir.join("nav.html"), "<nav>Links</nav>").unwrap();

        let result = render(
            &engine,
            &src_dir,
            "index.md",
            "Header {{.include nav.html}} Footer",
        )
        .unwrap();
        // HTML files should get padded with double newlines
        // And rendered in the markdown, standard tags like <nav> usually get passed through
        // We just check if it's there
        assert!(result.contains("<nav>Links</nav>"));
    }

    #[test]
    fn test_include_not_found() {
        let (_tmp, src_dir, engine) = setup_env();
        let err = render(&engine, &src_dir, "index.md", "{{.include nope.md}}").unwrap_err();
        match err.inner() {
            crate::ErrorKind::IncludeFileNotFound { include_path, .. } => {
                assert_eq!(include_path, "nope.md")
            }
            _ => panic!("Expected IncludeFileNotFound error"),
        }
    }

    #[test]
    fn test_include_path_traversal() {
        let (_tmp, src_dir, engine) = setup_env();
        let err = render(&engine, &src_dir, "index.md", "{{.include ../secret.txt}}").unwrap_err();
        match err.inner() {
            crate::ErrorKind::IncludePathTraversal { include_path, .. } => {
                assert_eq!(include_path, "../secret.txt")
            }
            _ => panic!("Expected IncludePathTraversal error"),
        }
    }

    #[test]
    fn test_max_recursion_depth() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(src_dir.join("a.md"), "{{.include b.md}}").unwrap();
        fs::write(src_dir.join("b.md"), "{{.include a.md}}").unwrap();

        let err = render(&engine, &src_dir, "index.md", "{{.include a.md}}").unwrap_err();
        match err.inner() {
            crate::ErrorKind::MaxRecursionDepthExceeded {
                depth, max_depth, ..
            } => {
                assert_eq!(*depth, 3);
                assert_eq!(*max_depth, 3);
            }
            _ => panic!("Expected MaxRecursionDepthExceeded error, got {:?}", err),
        }
    }

    #[test]
    fn test_unknown_directive() {
        let (_tmp, src_dir, engine) = setup_env();
        let err = render(&engine, &src_dir, "index.md", "{{.magic spell}}").unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnknownDirective { directive, .. } => {
                assert_eq!(directive, "magic spell")
            }
            _ => panic!("Expected UnknownDirective error"),
        }
    }

    #[test]
    fn test_unclosed_tag() {
        let (_tmp, src_dir, engine) = setup_env();
        let err = render(&engine, &src_dir, "index.md", "Text {{.var title").unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnclosedTag { .. } => {}
            _ => panic!("Expected UnclosedTag error"),
        }
    }

    #[test]
    fn test_include_with_default_arg_used_when_not_passed() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    name: \"stranger\"\n---\nHi {{.var name}}",
        )
        .unwrap();

        let result = render(&engine, &src_dir, "index.md", "{{.include greeting.md}}").unwrap();
        assert_eq!(result, "<p>Hi stranger</p>\n");
    }

    #[test]
    fn test_include_passed_literal_arg_overrides_default() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    name: \"stranger\"\n---\nHi {{.var name}}",
        )
        .unwrap();

        let result = render(
            &engine,
            &src_dir,
            "index.md",
            r#"{{.include greeting.md name="Bob"}}"#,
        )
        .unwrap();
        assert_eq!(result, "<p>Hi Bob</p>\n");
    }

    #[test]
    fn test_include_passed_variable_arg_resolves_from_parent_scope() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    name:\n---\nHi {{.var name}}",
        )
        .unwrap();

        let mut vars = HashMap::new();
        vars.insert("visitor".to_string(), "Alice".to_string());

        let result = render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            "{{.include greeting.md name=visitor}}",
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>Hi Alice</p>\n");
    }

    #[test]
    fn test_include_shorthand_arg_resolves_from_parent_scope() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    name:\n---\nHi {{.var name}}",
        )
        .unwrap();

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());

        let result = render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            "{{.include greeting.md name}}",
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>Hi Alice</p>\n");
    }

    #[test]
    fn test_include_missing_required_arg_errors() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    name:\n---\nHi {{.var name}}",
        )
        .unwrap();

        let err = render(&engine, &src_dir, "index.md", "{{.include greeting.md}}").unwrap_err();
        match err.inner() {
            crate::ErrorKind::MissingIncludeArgument {
                include_path,
                arg_name,
                ..
            } => {
                assert_eq!(arg_name, "name");
                assert_eq!(include_path, "greeting.md");
            }
            _ => panic!("Expected MissingIncludeArgument error, got {:?}", err),
        }
    }

    #[test]
    fn test_frontmatter_title_overrides_default_title() {
        let (_tmp, src_dir, engine) = setup_env();
        let mut vars = HashMap::new();
        render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            "---\ntitle: My Page\n---\nBody",
            &mut vars,
        )
        .unwrap();
        assert_eq!(vars.get("title"), Some(&"My Page".to_string()));
    }

    #[test]
    fn test_frontmatter_custom_vars_are_available_in_body() {
        let (_tmp, src_dir, engine) = setup_env();
        let result = render(
            &engine,
            &src_dir,
            "index.md",
            "---\ngreeting: Howdy\n---\n{{.var greeting}}",
        )
        .unwrap();
        assert_eq!(result, "<p>Howdy</p>\n");
    }

    #[test]
    fn test_if_else_conditional_rendering() {
        let (_tmp, src_dir, engine) = setup_env();
        let mut vars = HashMap::new();
        vars.insert("flag".to_string(), "1".to_string());
        let result = render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            "{{.if flag}}Yes{{.else}}No{{.end}}",
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>Yes</p>\n");
    }

    #[test]
    fn test_if_missing_var_renders_nothing_without_error() {
        let (_tmp, src_dir, engine) = setup_env();
        let result = render(&engine, &src_dir, "index.md", "{{.if flag}}Yes{{.end}}Done").unwrap();
        assert_eq!(result, "<p>Done</p>\n");
    }

    #[test]
    fn test_filter_upper_applied_in_rendered_output() {
        let (_tmp, src_dir, engine) = setup_env();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "world".to_string());
        let result = render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            "{{.var name | upper}}",
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>WORLD</p>\n");
    }

    #[test]
    fn test_include_arg_literal_interpolates_var_end_to_end() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    msg:\n---\n{{.var msg}}",
        )
        .unwrap();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        let result = render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            r#"{{.include greeting.md msg="Hi {{.var name}}"}}"#,
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>Hi Alice</p>\n");
    }

    #[test]
    fn test_include_arg_bareword_shorthand_with_filter_end_to_end() {
        // `{{.include greeter.md greeter|upper}}` — bareword shorthand
        // immediately followed by a filter, no whitespace before the `|`.
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(
            src_dir.join("greeter.md"),
            "---\nargs:\n    greeter:\n---\n{{.var greeter}}",
        )
        .unwrap();
        let mut vars = HashMap::new();
        vars.insert("greeter".to_string(), "world".to_string());
        let result = render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            "{{.include greeter.md greeter|upper}}",
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>WORLD</p>\n");
    }

    #[test]
    fn test_include_arg_key_value_with_filter_after_space_end_to_end() {
        // `{{.include greeter.md greeter=greeter |upper}}` — explicit
        // key=value pair, filter separated from the value by whitespace.
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(
            src_dir.join("greeter.md"),
            "---\nargs:\n    greeter:\n---\n{{.var greeter}}",
        )
        .unwrap();
        let mut vars = HashMap::new();
        vars.insert("greeter".to_string(), "world".to_string());
        let result = render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            "{{.include greeter.md greeter=greeter |upper}}",
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>WORLD</p>\n");
    }

    #[test]
    fn test_unknown_filter_errors_end_to_end() {
        let (_tmp, src_dir, engine) = setup_env();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "world".to_string());
        let err = render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            "{{.var name | bogus}}",
            &mut vars,
        )
        .unwrap_err();
        match err.inner() {
            crate::ErrorKind::UnknownFilter { filter_name, .. } => {
                assert_eq!(filter_name, "bogus")
            }
            _ => panic!("expected UnknownFilter, got {:?}", err),
        }
    }

    #[test]
    fn test_unclosed_if_errors_end_to_end() {
        let (_tmp, src_dir, engine) = setup_env();
        let err = render(&engine, &src_dir, "index.md", "{{.if flag}}content").unwrap_err();
        assert!(matches!(err.inner(), crate::ErrorKind::UnclosedIf { .. }));
    }

    #[test]
    fn test_if_interpolation_and_filters_compose() {
        // Exercises all three templating features together: `.if`/`.else`,
        // an include-arg literal that interpolates a `.var` tag, and that
        // tag itself using a `| title` filter.
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(
            src_dir.join("greeting.md"),
            "---\nargs:\n    msg:\n---\n{{.var msg}}",
        )
        .unwrap();

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "alice".to_string());
        let result = render_with_vars(
            &engine,
            &src_dir,
            "index.md",
            r#"{{.if !name}}Anonymous{{.else}}{{.include greeting.md msg="Hi {{.var name | title}}"}}{{.end}}"#,
            &mut vars,
        )
        .unwrap();
        assert_eq!(result, "<p>Hi Alice</p>\n");
    }

    #[test]
    fn test_block_and_end_directives_render_wrapping_div() {
        let (_tmp, src_dir, engine) = setup_env();
        let result = render(
            &engine,
            &src_dir,
            "index.md",
            "{{.block hero}}\n\ncontent\n\n{{.end}}",
        )
        .unwrap();
        assert!(result.contains("<div class=\"hero\">"));
        assert!(result.contains("</div>"));
    }

    #[test]
    fn test_block_directive_maps_name_to_configured_classes() {
        let (_tmp, src_dir, engine) = setup_env();

        let mut options = engine.options.clone();
        options
            .blocks
            .insert("hero".to_string(), "bg-hero p-8".to_string());
        let engine: RenderEngine<T> = RenderEngine::new(engine.paths.clone(), options);

        let result = render(
            &engine,
            &src_dir,
            "index.md",
            "{{.block hero}}\n\ncontent\n\n{{.end}}",
        )
        .unwrap();
        assert!(result.contains("<div class=\"bg-hero p-8\">"));
    }

    #[test]
    fn test_resolve_paths_returns_none_for_nonexistent_route() {
        let (_tmp, _src_dir, engine) = setup_env();
        assert!(engine.resolve_paths("/does-not-exist").is_none());
    }

    #[test]
    fn test_resolve_paths_strips_slashes_and_extensions() {
        let (_tmp, src_dir, engine) = setup_env();
        let about_dir = src_dir.join("about");
        fs::create_dir_all(&about_dir).unwrap();
        fs::write(about_dir.join("index.md"), "About").unwrap();

        let (route_dir, src_md, public_html) = engine.resolve_paths("/about/").unwrap();
        assert_eq!(route_dir, about_dir);
        assert_eq!(src_md, about_dir.join("index.md"));
        assert!(public_html.ends_with("about/index.html"));

        // `.html` and `.md` suffixes on the URI should resolve the same route.
        let (route_dir2, ..) = engine.resolve_paths("/about.html").unwrap();
        assert_eq!(route_dir2, about_dir);
        let (route_dir3, ..) = engine.resolve_paths("/about.md").unwrap();
        assert_eq!(route_dir3, about_dir);
    }

    #[test]
    fn test_resolve_paths_rejects_parent_dir_traversal() {
        let (_tmp, src_dir, engine) = setup_env();

        // A route resolving to a page outside `src_dir` via `..` segments
        // must be rejected, even when a real file sits at the escaped
        // location — otherwise the lexical `starts_with` check alone can be
        // fooled, since `PathBuf::join` doesn't resolve `..` components.
        let base_path = src_dir.parent().unwrap().parent().unwrap();
        let secret_dir = base_path.join("secret");
        fs::create_dir_all(&secret_dir).unwrap();
        fs::write(secret_dir.join("index.md"), "top secret").unwrap();

        assert!(engine.resolve_paths("../../secret").is_none());
        assert!(engine.resolve_paths("/about/../../../secret").is_none());
    }

    #[test]
    fn test_resolve_paths_root_uri_maps_to_src_dir() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(src_dir.join("index.md"), "Home").unwrap();

        let (route_dir, src_md, _public_html) = engine.resolve_paths("/").unwrap();
        assert_eq!(route_dir, src_dir);
        assert_eq!(src_md, src_dir.join("index.md"));
    }

    #[test]
    fn test_is_stale_true_when_output_missing() {
        let (_tmp, src_dir, engine) = setup_env();
        let missing_output = engine.paths.public_dir.join("index.html");
        assert!(engine.is_stale(&src_dir, &missing_output));
    }

    #[test]
    fn test_is_stale_false_when_output_is_newer_than_sources() {
        let (_tmp, src_dir, engine) = setup_env();
        fs::write(src_dir.join("index.md"), "Home").unwrap();

        let public_html = engine.paths.public_dir.join("index.html");
        let public_css = engine.paths.public_dir.join("assets/style.css");
        fs::create_dir_all(&engine.paths.public_dir).unwrap();
        fs::create_dir_all(engine.paths.public_dir.join("assets")).unwrap();
        fs::write(&public_html, "<p>Home</p>").unwrap();
        fs::write(public_css, "").unwrap();

        assert!(!engine.is_stale(&src_dir, &public_html));
    }

    #[test]
    fn test_compile_page_writes_output_to_disk() {
        let (_tmp, src_dir, engine) = setup_env();
        let index = src_dir.join("index.md");
        fs::write(&index, "Hello").unwrap();

        let public_html = engine.paths.public_dir.join("index.html");
        let mut vars = HashMap::new();
        engine
            .compile_page(&index, &public_html, &mut vars)
            .unwrap();

        assert!(public_html.exists());
        let contents = fs::read_to_string(&public_html).unwrap();
        assert!(contents.contains("Hello"));
    }

    #[cfg(feature = "detailed-errors")]
    #[test]
    fn test_format_error_detailed_includes_help_text() {
        let (_tmp, src_dir, mut engine) = setup_env();
        let index = src_dir.join("index.md");
        fs::write(&index, "{{.var missing}}").unwrap();

        let mut vars = HashMap::new();
        let err = engine.render_page(&index, &mut vars).unwrap_err();

        engine.options.detailed_errors = false;
        let plain = engine.format_error(crate::Error::custom("boom".to_string()));
        assert_eq!(plain, "boom");

        engine.options.detailed_errors = true;
        let detailed = engine.format_error(err);
        assert!(detailed.contains("Make sure"));
    }
}
