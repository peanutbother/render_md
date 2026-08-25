/// Defines the errors that can occur during template parsing and rendering.
#[derive(
    Debug,
    thiserror::Error,
    miette::Diagnostic,
    thiserror_ext::Box,
    thiserror_ext::Construct,
    thiserror_ext::Macro,
)]
#[thiserror_ext(newtype(name = Error))]
pub enum ErrorKind {
    /// An underlying IO error.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// An error originating from `gray_matter` when parsing front matter.
    #[error(transparent)]
    GrayMatter(#[from] gray_matter::Error),

    /// An error originating from `miette`, e.g. when initializing report handler.
    #[error("{0}")]
    Miette(miette::Error),

    /// A referenced variable was missing from the rendering context.
    #[error("Variable '{var_name}' was not found in context")]
    #[diagnostic(
        code(renderer::template::variable_not_found),
        help(
            "Make sure '{var_name}' is inserted into the template context HashMap before rendering."
        )
    )]
    VariableNotFound {
        /// The name of the variable that could not be found.
        var_name: String,
        /// The source code where the variable was referenced.
        #[source_code]
        src: miette::NamedSource<String>,
        /// The location of the reference in the source code.
        #[label("this variable reference is undefined")]
        span: miette::SourceSpan,
    },

    /// An included file could not be found on the filesystem.
    #[error("Failed to resolve include path '{include_path}'")]
    #[diagnostic(
        code(renderer::template::include_file_not_found),
        help("Check that '{include_path}' exists relative to the parent file.")
    )]
    IncludeFileNotFound {
        /// The path of the include that failed to resolve.
        include_path: String,
        /// The source code where the include directive was found.
        #[source_code]
        src: miette::NamedSource<String>,
        /// The location of the include directive in the source code.
        #[label("this include path does not exist on disk")]
        span: miette::SourceSpan,
    },

    /// An attempt was made to include a file outside the allowed source directory.
    #[error("Include path escape attempt or outside src directory: '{include_path}'")]
    #[diagnostic(
        code(renderer::template::include_path_traversal),
        help(
            "Includes must stay within the 'include_path' directory and cannot traverse outside using '..'."
        )
    )]
    IncludePathTraversal {
        /// The path that attempted directory traversal.
        include_path: String,
        /// The source code containing the invalid path.
        #[source_code]
        src: miette::NamedSource<String>,
        /// The location of the invalid include directive.
        #[label("this include path attempts directory traversal")]
        span: miette::SourceSpan,
    },

    /// The maximum depth for nested includes was exceeded.
    #[error("Maximum include recursion depth exceeded (limit: {max_depth})")]
    #[diagnostic(
        code(renderer::template::max_recursion_depth),
        help("Check for circular file inclusions between markdown partials.")
    )]
    MaxRecursionDepthExceeded {
        /// The source code where the limit was reached.
        #[source_code]
        src: miette::NamedSource<String>,
        /// The location of the include directive that caused the limit to be exceeded.
        #[label("recursion limit reached here")]
        span: miette::SourceSpan,
        /// The include depth reached when the limit was hit.
        depth: usize,
        /// The configured maximum recursion depth limit.
        max_depth: usize,
    },

    /// An unknown template directive was encountered.
    #[error("Unknown template directive '{{.{directive}}}'")]
    #[diagnostic(
        code(renderer::template::unknown_directive),
        help(
            "Valid directives are '{{.var NAME}}', '{{.include PATH}}', '{{.block NAME}}', '{{.if CONDITION}}' and '{{.else}}'."
        )
    )]
    UnknownDirective {
        /// The unknown directive name.
        directive: String,
        /// The source code containing the invalid directive.
        #[source_code]
        src: miette::NamedSource<String>,
        /// The location of the invalid directive.
        #[label("unknown directive used here")]
        span: miette::SourceSpan,
    },

    /// A template tag was opened but never closed.
    #[error("Unclosed template tag starting with '{{.'")]
    #[diagnostic(
        code(renderer::template::unclosed_tag),
        help("Template tags must be closed with '}}'. Check for missing closing braces.")
    )]
    UnclosedTag {
        /// The source code containing the unclosed tag.
        #[source_code]
        src: miette::NamedSource<String>,
        /// The location where the unclosed tag starts.
        #[label("unclosed tag starts here")]
        span: miette::SourceSpan,
    },

    /// A block directive was used without providing a block name.
    #[error("Missing block name in '{{{{.block}}}}' directive")]
    #[diagnostic(
        code(renderer::template::missing_block_name),
        help("Provide a block name, e.g., '{{{{.block my_block}}}}'.")
    )]
    MissingBlockName {
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("block name missing here")]
        span: miette::SourceSpan,
    },

    /// A block tag was opened but never closed.
    #[error("Unclosed block directive")]
    #[diagnostic(
        code(renderer::template::unclosed_block),
        help("Ensure every '{{{{.block ...}}}}' has a matching '{{{{.end}}}}'.")
    )]
    UnclosedBlock {
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("block started here")]
        span: miette::SourceSpan,
    },

    /// An include directive was used without providing a file name.
    #[error("Missing file name in '{{{{.include}}}}' directive")]
    #[diagnostic(
        code(renderer::template::missing_include_name),
        help("Provide a file name, e.g., '{{{{.include my_file.md}}}}'.")
    )]
    MissingIncludeName {
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("file name missing here")]
        span: miette::SourceSpan,
    },

    /// An include directive was missing a required argument.
    #[error("Missing required argument '{arg_name}' for include '{include_path}'")]
    #[diagnostic(
        code(renderer::template::missing_include_argument),
        help(
            "Provide the argument, e.g., '{{{{.include {include_path} {arg_name}=\"value\"}}}}'."
        )
    )]
    MissingIncludeArgument {
        include_path: String,
        arg_name: String,
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("argument '{arg_name}' is required by the included file")]
        span: miette::SourceSpan,
    },

    /// An `.if` directive was used without providing a condition.
    #[error("Missing condition in '{{{{.if}}}}' directive")]
    #[diagnostic(
        code(renderer::template::missing_if_condition),
        help(
            "Provide a condition, e.g., '{{{{.if my_var}}}}' or '{{{{.if my_var == \"value\"}}}}'."
        )
    )]
    MissingIfCondition {
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("condition missing here")]
        span: miette::SourceSpan,
    },

    /// An `.if` directive's condition text could not be parsed.
    #[error("Invalid condition in '{{{{.if}}}}' directive: {reason}")]
    #[diagnostic(
        code(renderer::template::invalid_condition),
        help(
            "Conditions support 'var', '!var', 'var == \"literal\"' and 'var != other_var' forms."
        )
    )]
    InvalidCondition {
        reason: String,
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("invalid condition here")]
        span: miette::SourceSpan,
    },

    /// An `.if` tag was opened but never closed.
    #[error("Unclosed if directive")]
    #[diagnostic(
        code(renderer::template::unclosed_if),
        help("Ensure every '{{{{.if ...}}}}' has a matching '{{{{.end}}}}'.")
    )]
    UnclosedIf {
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("if started here")]
        span: miette::SourceSpan,
    },

    /// A `.var` directive referenced an unknown filter after a `|`.
    #[error("Unknown filter '{filter_name}' in '{{{{.var}}}}' directive")]
    #[diagnostic(
        code(renderer::template::unknown_filter),
        help("Valid filters are 'upper', 'lower', 'trim' and 'title'.")
    )]
    UnknownFilter {
        filter_name: String,
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("unknown filter used here")]
        span: miette::SourceSpan,
    },

    /// Compilation of Tailwind CSS styles failed.
    #[error("Failed to compile Tailwind stylesheet: {0}")]
    #[diagnostic(
        code(renderer::tailwind_compilation_failed),
        help("Ensure Tailwind CLI is installed and configured correctly.")
    )]
    TailwindCompilationFailed(String),

    /// The Tailwind CLI process could not be launched at all (e.g. binary
    /// missing or not executable) — distinct from `TailwindCompilationFailed`,
    /// which covers the process running but exiting with a failure status.
    #[error("Failed to execute tailwindcss binary: {0}")]
    #[diagnostic(
        code(renderer::tailwind_execution_failed),
        help("Ensure the tailwindcss binary (see TAILWIND_BIN) is installed and on PATH.")
    )]
    TailwindExecutionFailed(String),

    /// A custom error that does not fit into other variants.
    #[error("{0}")]
    #[diagnostic(code(renderer::custom))]
    Custom(#[message] String),
}
