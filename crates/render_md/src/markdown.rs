use pulldown_cmark::{Options, Parser, html};
use std::collections::HashMap;

/// Builds the opening HTML tag for a styled block, if `key` has a
/// configured class. `trailing_nl` controls whether a newline follows the
/// tag (matches each element's existing formatting: `blockquote`/lists get
/// one, `p`/`li` don't).
fn styled_open<'a>(
    active_tags: &HashMap<String, String>,
    key: &str,
    tag: &str,
    trailing_nl: bool,
) -> Option<pulldown_cmark::Event<'a>> {
    active_tags.get(key).map(|c| {
        let nl = if trailing_nl { "\n" } else { "" };
        pulldown_cmark::Event::Html(format!("<{tag} class=\"{c}\">{nl}").into())
    })
}

/// Builds the closing HTML tag for a styled block, if `key` has a
/// configured class. Every closing tag in this file gets a trailing newline.
fn styled_close<'a>(
    active_tags: &HashMap<String, String>,
    key: &str,
    tag: &str,
) -> Option<pulldown_cmark::Event<'a>> {
    active_tags
        .contains_key(key)
        .then(|| pulldown_cmark::Event::Html(format!("</{tag}>\n").into()))
}

/// Uses [pulldown_cmark::Parser] to parse content with custom [pulldown_cmark::Event] handling
pub fn render_markdown(processed_content: &str, active_tags: &HashMap<String, String>) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options.insert(Options::ENABLE_WIKILINKS);

    let parser = Parser::new_ext(processed_content, options);

    let parser = parser.map(|event| match event {
        // Headings are structurally different from everything else below:
        // pulldown renders the tag itself, so we append the class onto its
        // own `classes` list rather than emitting raw HTML.
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::Heading {
            level,
            id,
            mut classes,
            ..
        }) => {
            let key = format!("h{}", level as usize);
            if let Some(c) = active_tags.get(&key) {
                classes.push(c.to_string().into());
            }
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Heading {
                level,
                id,
                classes,
                attrs: vec![],
            })
        }
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::Paragraph)
            if active_tags.contains_key("p") =>
        {
            styled_open(active_tags, "p", "p", false).unwrap()
        }
        pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Paragraph)
            if active_tags.contains_key("p") =>
        {
            styled_close(active_tags, "p", "p").unwrap()
        }
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::BlockQuote(_))
            if active_tags.contains_key("blockquote") =>
        {
            styled_open(active_tags, "blockquote", "blockquote", true).unwrap()
        }
        pulldown_cmark::Event::End(pulldown_cmark::TagEnd::BlockQuote(_))
            if active_tags.contains_key("blockquote") =>
        {
            styled_close(active_tags, "blockquote", "blockquote").unwrap()
        }
        // Code blocks need the fenced language baked into the opening tag,
        // so (like headings) they stay a bespoke arm rather than going
        // through the generic `styled_open` helper.
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(kind))
            if let Some(c) = active_tags.get("code") =>
        {
            let lang_attr = match kind {
                pulldown_cmark::CodeBlockKind::Fenced(lang) if !lang.is_empty() => {
                    format!(" class=\"language-{lang}\"")
                }
                _ => String::new(),
            };
            pulldown_cmark::Event::Html(format!("<pre class=\"{c}\"><code{lang_attr}>").into())
        }
        pulldown_cmark::Event::End(pulldown_cmark::TagEnd::CodeBlock)
            if active_tags.contains_key("code") =>
        {
            pulldown_cmark::Event::Html("</code></pre>\n".into())
        }
        // Ordered (`ol`) and unordered (`ul`) lists share one arm; `start`
        // (`Some` for ordered lists) both picks the tag/class key and
        // controls the `start="N"` attribute.
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::List(start)) => {
            let key = if start.is_some() { "ol" } else { "ul" };
            match active_tags.get(key) {
                Some(c) => {
                    let start_attr = start.map(|n| format!(" start=\"{n}\"")).unwrap_or_default();
                    pulldown_cmark::Event::Html(
                        format!("<{key} class=\"{c}\"{start_attr}>\n").into(),
                    )
                }
                None => pulldown_cmark::Event::Start(pulldown_cmark::Tag::List(start)),
            }
        }
        pulldown_cmark::Event::End(pulldown_cmark::TagEnd::List(ordered)) => {
            let key = if ordered { "ol" } else { "ul" };
            styled_close(active_tags, key, key).unwrap_or(pulldown_cmark::Event::End(
                pulldown_cmark::TagEnd::List(ordered),
            ))
        }
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::Item)
            if active_tags.contains_key("li") =>
        {
            styled_open(active_tags, "li", "li", false).unwrap()
        }
        pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Item)
            if active_tags.contains_key("li") =>
        {
            styled_close(active_tags, "li", "li").unwrap()
        }
        _ => event,
    });

    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_plain_paragraph_no_active_tags() {
        let result = render_markdown("Hello world", &HashMap::new());
        assert_eq!(result, "<p>Hello world</p>\n");
    }

    #[test]
    fn test_heading_gets_class_for_its_level() {
        let active = tags(&[("h1", "text-4xl"), ("h2", "text-2xl")]);
        let result = render_markdown("# Title\n\n## Subtitle", &active);
        assert!(result.contains("<h1 class=\"text-4xl\">Title</h1>"));
        assert!(result.contains("<h2 class=\"text-2xl\">Subtitle</h2>"));
    }

    #[test]
    fn test_heading_without_matching_tag_is_unstyled() {
        let active = tags(&[("h2", "text-2xl")]);
        let result = render_markdown("# Title", &active);
        assert_eq!(result, "<h1>Title</h1>\n");
    }

    #[test]
    fn test_paragraph_gets_class() {
        let active = tags(&[("p", "leading-6")]);
        let result = render_markdown("Hello world", &active);
        assert_eq!(result, "<p class=\"leading-6\">Hello world</p>\n");
    }

    #[test]
    fn test_blockquote_gets_class() {
        let active = tags(&[("blockquote", "italic")]);
        let result = render_markdown("> Quoted text", &active);
        assert!(result.contains("<blockquote class=\"italic\">\n"));
        assert!(result.contains("</blockquote>\n"));
    }

    #[test]
    fn test_fenced_code_block_gets_class_and_language() {
        let active = tags(&[("code", "overflow-x-auto")]);
        let result = render_markdown("```rust\nfn main() {}\n```", &active);
        assert!(result.contains("<pre class=\"overflow-x-auto\"><code class=\"language-rust\">"));
        assert!(result.contains("</code></pre>\n"));
    }

    #[test]
    fn test_code_block_without_language_omits_class_attr() {
        let active = tags(&[("code", "overflow-x-auto")]);
        let result = render_markdown("```\nplain\n```", &active);
        assert!(result.contains("<pre class=\"overflow-x-auto\"><code>"));
    }

    #[test]
    fn test_unordered_list_and_items_get_classes() {
        let active = tags(&[("ul", "list-disc"), ("li", "ml-4")]);
        let result = render_markdown("- one\n- two", &active);
        assert!(result.contains("<ul class=\"list-disc\">\n"));
        assert!(result.contains("<li class=\"ml-4\">one</li>\n"));
        assert!(result.contains("</ul>\n"));
    }

    #[test]
    fn test_ordered_list_gets_class_and_start() {
        let active = tags(&[("ol", "list-decimal")]);
        let result = render_markdown("3. three\n4. four", &active);
        assert!(result.contains("<ol class=\"list-decimal\" start=\"3\">\n"));
        assert!(result.contains("</ol>\n"));
    }

    #[test]
    fn test_table_extension_is_enabled() {
        let result = render_markdown("| a | b |\n|---|---|\n| 1 | 2 |", &HashMap::new());
        assert!(result.contains("<table>"));
    }

    #[test]
    fn test_strikethrough_extension_is_enabled() {
        let result = render_markdown("~~gone~~", &HashMap::new());
        assert!(result.contains("<del>gone</del>"));
    }

    #[test]
    fn test_tasklist_extension_is_enabled() {
        let result = render_markdown("- [x] done", &HashMap::new());
        assert!(result.contains("type=\"checkbox\""));
        assert!(result.contains("checked"));
    }
}
