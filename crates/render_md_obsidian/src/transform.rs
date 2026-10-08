//! The note transform: step 3 (wikilinks, embed placeholders, heading ids,
//! block ids, tags, query blocks), step 4 (callouts, see
//! [`crate::callouts`]) and step 5 (embed expansion), plus properties,
//! bases and kanban boards.

use crate::bases::{Base, column_key};
use crate::callouts;
use crate::diagnostics::Diagnostics;
use crate::glob::Glob;
use crate::resolve::Resolution;
use crate::scan::{self, IdInsert, Item, Link};
use crate::slug::{self, IdRegistry};
use crate::text::{
    continuation_prefix, escape_html, escape_markdown, indent_continuation, line_end, line_start,
    splice,
};
use crate::vault::{self, FileId, Note, NoteId, Vault, property_strings, tag_route};
use render_md::gray_matter::Pod;
use std::collections::BTreeSet;
use std::ops::Range;

/// How `![[note]]` embeds of whole notes are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbedMode {
    /// In a closed `<details>`, titled with a link to the note.
    #[default]
    Collapsed,
    /// Inline, in a bordered block.
    Inline,
}

/// Properties never shown in a note's properties table.
pub const DEFAULT_HIDDEN_PROPERTIES: &[&str] = &[
    "aliases",
    "alias",
    "blueprint",
    "cssclasses",
    "cssclass",
    "kanban-plugin",
    "position",
    "publish",
    "permalink",
    "title",
];

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "avif", "bmp"];
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "m4a", "flac", "webm"];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "ogv"];

pub struct Transformer<'a> {
    pub vault: &'a Vault,
    pub embeds: EmbedMode,
    pub max_embed_depth: usize,
    hidden: Vec<Glob>,
}

/// The result of transforming one note into its page body.
#[derive(Debug, Default)]
pub struct TransformedNote {
    pub body: String,
    /// The properties table (HTML), if the note has visible properties.
    pub properties: Option<String>,
    /// Notes linked from this note's own text and properties.
    pub links: BTreeSet<NoteId>,
    /// Vault files that have to be published.
    pub files: BTreeSet<FileId>,
}

/// State shared by a page and everything embedded into it.
struct Page {
    ids: IdRegistry,
    /// Embeds being expanded: `(note, section anchor)`.
    stack: Vec<(NoteId, Option<String>)>,
    links: BTreeSet<NoteId>,
    files: BTreeSet<FileId>,
}

/// A step-3 placeholder for an embed, replaced in step 5. Private-use
/// characters can't collide with anything a note plausibly contains.
fn placeholder(n: usize) -> String {
    format!("\u{E000}embed{n}\u{E001}")
}

pub fn tag_html(tag: &str) -> String {
    format!(
        "<a class=\"tag\" href=\"{}\">#{}</a>",
        escape_html(&tag_route(tag)),
        escape_html(tag)
    )
}

impl<'a> Transformer<'a> {
    pub fn new(
        vault: &'a Vault,
        embeds: EmbedMode,
        max_embed_depth: usize,
        hidden_properties: &[String],
    ) -> Self {
        Self {
            vault,
            embeds,
            max_embed_depth,
            hidden: DEFAULT_HIDDEN_PROPERTIES
                .iter()
                .copied()
                .chain(hidden_properties.iter().map(String::as_str))
                .map(Glob::new)
                .collect(),
        }
    }

    pub fn note(&self, id: NoteId, diags: &mut Diagnostics) -> TransformedNote {
        let mut page = Page {
            ids: IdRegistry::default(),
            stack: vec![(id, None)],
            links: BTreeSet::new(),
            files: BTreeSet::new(),
        };
        let note = &self.vault.notes[id];
        let body = self.text(&mut page, id, &note.body, "", 0, diags);
        let properties = self.properties_table(&mut page, id, diags);
        page.links.remove(&id);
        TransformedNote {
            body,
            properties,
            links: page.links,
            files: page.files,
        }
    }

    fn source(&self, id: NoteId) -> &str {
        &self.vault.notes[id].path
    }

    /// Steps 3–5 for `text`, a note's body or a section of it. `prefix` is
    /// prepended to heading ids of embedded content; `depth` is 0 for the
    /// page's own note.
    fn text(
        &self,
        page: &mut Page,
        note: NoteId,
        text: &str,
        prefix: &str,
        depth: usize,
        diags: &mut Diagnostics,
    ) -> String {
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        let mut embeds: Vec<Link> = Vec::new();
        let mut lanes: Vec<usize> = Vec::new();

        for item in scan::scan(text) {
            match item {
                Item::WikiLink(link) => {
                    let replacement = self.link(page, note, &link, depth, diags);
                    edits.push((link.range, replacement));
                }
                Item::Embed(link) => {
                    edits.push((link.range.clone(), placeholder(embeds.len())));
                    embeds.push(link);
                }
                Item::Heading(heading) => {
                    if heading.top_level && heading.level == 2 {
                        lanes.push(line_start(text, heading.range.start));
                    }
                    let id = vault::heading_id(&mut page.ids, prefix, &heading);
                    match heading.insert {
                        IdInsert::Append(pos) => edits.push((pos..pos, format!(" {{#{id}}}"))),
                        IdInsert::IntoBraces(pos) => edits.push((pos..pos, format!(" #{id}"))),
                        IdInsert::Keep => {}
                    }
                }
                Item::BlockId { range, id } => {
                    let anchor = if depth == 0 {
                        let space = if text[range.clone()].starts_with('^') {
                            ""
                        } else {
                            " "
                        };
                        format!("{space}<a id=\"^{id}\" class=\"block-id\"></a>")
                    } else {
                        String::new()
                    };
                    edits.push((range, anchor));
                }
                Item::Tag { range, tag } => edits.push((range, tag_html(&tag))),
                Item::Query { range, lang } => {
                    let replacement = query_block(text, range.clone(), &lang);
                    edits.push((range, replacement));
                }
            }
        }

        if depth == 0 && self.vault.notes[note].kanban && !lanes.is_empty() {
            for (i, start) in lanes.iter().enumerate() {
                let open = if i == 0 {
                    "<div class=\"kanban\">\n\n<section class=\"kanban-lane\">\n\n"
                } else {
                    "\n\n</section>\n\n<section class=\"kanban-lane\">\n\n"
                };
                edits.push((*start..*start, open.to_owned()));
            }
            edits.push((
                text.len()..text.len(),
                "\n\n</section>\n\n</div>\n".to_owned(),
            ));
        }

        let text = splice(text, edits);
        let text = callouts::transform(&text);
        self.expand_embeds(page, note, &text, &embeds, depth, diags)
    }

    /// A wikilink as a Markdown link (or text, for excluded targets, or a
    /// marked span, for broken ones).
    fn link(
        &self,
        page: &mut Page,
        from: NoteId,
        link: &Link,
        depth: usize,
        diags: &mut Diagnostics,
    ) -> String {
        let display = link.display();
        match self.resolve(page, from, &link.target, depth, diags) {
            Some(url) => format!("[{}](<{url}>)", escape_markdown(&display)),
            None if self.vault.resolve(from, &link.target) == Resolution::Unresolved => {
                broken_span(&display, &link.target)
            }
            None => escape_markdown(&display),
        }
    }

    /// Resolves a link target to a URL, reporting problems. `None` for
    /// targets without a page (unresolved, excluded, bases).
    fn resolve(
        &self,
        page: &mut Page,
        from: NoteId,
        target: &str,
        depth: usize,
        diags: &mut Diagnostics,
    ) -> Option<String> {
        let source = self.source(from);
        match self.vault.resolve(from, target) {
            Resolution::Note {
                id,
                anchor,
                missing_fragment,
                ambiguous,
            } => {
                if let Some(fragment) = missing_fragment {
                    diags.broken(
                        source,
                        format!(
                            "[[{target}]]: '{fragment}' is not a heading or block in '{}'",
                            self.vault.notes[id].path
                        ),
                    );
                }
                if !ambiguous.is_empty() {
                    diags.warn(
                        source,
                        format!(
                            "[[{target}]] is ambiguous, linking '{}' (also: {})",
                            self.vault.notes[id].path,
                            ambiguous.join(", ")
                        ),
                    );
                }
                if depth == 0 {
                    page.links.insert(id);
                }
                let url = match anchor {
                    Some(anchor) if id == from && depth == 0 => format!("#{anchor}"),
                    Some(anchor) => format!("{}#{anchor}", self.vault.notes[id].route),
                    None => self.vault.notes[id].route.clone(),
                };
                Some(url)
            }
            Resolution::File(id) => {
                let file = &self.vault.files[id];
                if file.extension == "base" {
                    return None;
                }
                page.files.insert(id);
                Some(file.url.clone())
            }
            Resolution::Excluded => None,
            Resolution::Unresolved => {
                diags.broken(source, format!("unresolved link [[{target}]]"));
                None
            }
        }
    }

    /// Step 5: replaces the placeholders of step 3.
    fn expand_embeds(
        &self,
        page: &mut Page,
        note: NoteId,
        text: &str,
        embeds: &[Link],
        depth: usize,
        diags: &mut Diagnostics,
    ) -> String {
        let mut edits = Vec::new();
        for (n, link) in embeds.iter().enumerate() {
            let token = placeholder(n);
            let Some(pos) = text.find(&token) else {
                continue;
            };
            let start = line_start(text, pos);
            let end = line_end(text, pos);
            let before = &text[start..pos];
            let after = &text[pos + token.len()..end];
            if after.trim().is_empty() && is_block_prefix(before) {
                let block = self.embed_block(page, note, link, depth, diags);
                let prefix = continuation_prefix(before);
                let blank = prefix.trim_end();
                let mut replacement = String::new();
                let range = if before.chars().all(|c| matches!(c, ' ' | '\t' | '>')) {
                    // Blank lines around the block so it never joins a paragraph.
                    replacement.push_str(blank);
                    replacement.push('\n');
                    replacement.push_str(before);
                    start..end
                } else {
                    pos..end
                };
                replacement.push_str(&indent_continuation(&block, &prefix));
                replacement.push('\n');
                replacement.push_str(blank);
                edits.push((range, replacement));
            } else {
                let inline = self.embed_inline(page, note, link, depth, diags);
                edits.push((pos..pos + token.len(), inline));
            }
        }
        splice(text, edits)
    }

    /// An embed alone on its line.
    fn embed_block(
        &self,
        page: &mut Page,
        from: NoteId,
        link: &Link,
        depth: usize,
        diags: &mut Diagnostics,
    ) -> String {
        let source = self.source(from).to_owned();
        let display = link.display();
        let fragment = link.target.find('#').map(|i| link.target[i + 1..].trim());
        match self.vault.resolve(from, &link.target) {
            Resolution::Note {
                id,
                anchor,
                missing_fragment,
                ..
            } => {
                if depth == 0 {
                    page.links.insert(id);
                }
                let note = &self.vault.notes[id];
                if let Some(fragment) = missing_fragment {
                    diags.broken(
                        &source,
                        format!(
                            "![[{}]]: '{fragment}' is not a heading or block in '{}'",
                            link.target, note.path
                        ),
                    );
                    return broken_block(&display, &link.target);
                }
                let key = (id, anchor.clone());
                if page.stack.contains(&key) {
                    diags.broken(&source, format!("![[{}]] embeds itself", link.target));
                    return format!(
                        "<p>{}</p>",
                        self.note_anchor(id, anchor.as_deref(), &display)
                    );
                }
                if depth + 1 > self.max_embed_depth {
                    diags.broken(
                        &source,
                        format!(
                            "![[{}]] is nested deeper than {} embeds",
                            link.target, self.max_embed_depth
                        ),
                    );
                    return format!(
                        "<p>{}</p>",
                        self.note_anchor(id, anchor.as_deref(), &display)
                    );
                }
                let prefix = format!("embed-{}-", slug::heading_id(&note.name));
                match anchor.as_deref() {
                    None if fragment.is_some_and(|f| !f.is_empty()) => {
                        // A fragment that resolved to nothing usable.
                        format!("<p>{}</p>", self.note_anchor(id, None, &display))
                    }
                    None => {
                        page.stack.push(key);
                        let inner = self.text(page, id, &note.body, &prefix, depth + 1, diags);
                        page.stack.pop();
                        self.whole_note_embed(id, &display, &inner)
                    }
                    Some(block) if block.starts_with('^') => {
                        diags.warn(
                            &source,
                            format!("![[{}]]: block embeds are shown as links", link.target),
                        );
                        format!("<p>{}</p>", self.note_anchor(id, Some(block), &display))
                    }
                    Some(heading_id) => {
                        let Some(range) = section_range(note, heading_id) else {
                            diags.warn(
                                &source,
                                format!(
                                    "![[{}]]: sections inside lists or quotes are shown as links",
                                    link.target
                                ),
                            );
                            return format!(
                                "<p>{}</p>",
                                self.note_anchor(id, Some(heading_id), &display)
                            );
                        };
                        page.stack.push(key);
                        let inner =
                            self.text(page, id, &note.body[range], &prefix, depth + 1, diags);
                        page.stack.pop();
                        embed_wrapper(
                            "div",
                            "embed embed-section",
                            &self.note_anchor(id, Some(heading_id), &display),
                            &inner,
                        )
                    }
                }
            }
            Resolution::File(id) => {
                let file = &self.vault.files[id];
                if file.extension == "base" {
                    return self.base_embed(page, from, id, fragment, depth, diags);
                }
                page.files.insert(id);
                file_embed(&self.vault.files[id], link)
            }
            Resolution::Excluded => format!("<p>{}</p>", escape_html(&display)),
            Resolution::Unresolved => {
                diags.broken(&source, format!("unresolved embed ![[{}]]", link.target));
                broken_block(&display, &link.target)
            }
        }
    }

    /// An embed inside running text: images stay images, everything else
    /// becomes a link.
    fn embed_inline(
        &self,
        page: &mut Page,
        from: NoteId,
        link: &Link,
        depth: usize,
        diags: &mut Diagnostics,
    ) -> String {
        if let Resolution::File(id) = self.vault.resolve(from, &link.target) {
            let file = &self.vault.files[id];
            if IMAGE_EXTENSIONS.contains(&file.extension.as_str()) {
                page.files.insert(id);
                return file_embed(file, link);
            }
        }
        let display = link.display();
        match self.resolve(page, from, &link.target, depth, diags) {
            Some(url) => format!(
                "<a class=\"embed-link\" href=\"{}\">{}</a>",
                escape_html(&url),
                escape_html(&display)
            ),
            None if self.vault.resolve(from, &link.target) == Resolution::Unresolved => {
                broken_span(&display, &link.target)
            }
            None => escape_html(&display),
        }
    }

    fn note_anchor(&self, id: NoteId, anchor: Option<&str>, display: &str) -> String {
        let route = &self.vault.notes[id].route;
        let url = match anchor {
            Some(anchor) => format!("{route}#{anchor}"),
            None => route.clone(),
        };
        format!(
            "<a href=\"{}\">{}</a>",
            escape_html(&url),
            escape_html(display)
        )
    }

    fn whole_note_embed(&self, id: NoteId, display: &str, inner: &str) -> String {
        let title = self.note_anchor(id, None, display);
        match self.embeds {
            EmbedMode::Collapsed => embed_wrapper("details", "embed", &title, inner),
            EmbedMode::Inline => embed_wrapper("div", "embed", &title, inner),
        }
    }

    fn base_embed(
        &self,
        page: &mut Page,
        from: NoteId,
        file: FileId,
        view_name: Option<&str>,
        depth: usize,
        diags: &mut Diagnostics,
    ) -> String {
        let base_file = &self.vault.files[file];
        let unsupported = |diags: &mut Diagnostics, reason: String| {
            diags.warn(&base_file.path, reason.clone());
            format!(
                "<p class=\"base-unsupported\">{} can't be shown here: {}</p>",
                escape_html(&base_file.name),
                escape_html(&reason)
            )
        };
        let path = self.vault.root.join(&base_file.path);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => return unsupported(diags, format!("failed to read it: {err}")),
        };
        let base = match Base::parse(&text) {
            Ok(base) => base,
            Err(err) => return unsupported(diags, err),
        };
        let Some(view) = base.view(view_name.filter(|v| !v.is_empty())) else {
            return unsupported(diags, "it has no table view".to_owned());
        };
        let columns: Vec<String> = if view.order.is_empty() {
            vec!["file.name".to_owned()]
        } else {
            view.order.clone()
        };

        let mut rows: Vec<(NoteId, Vec<String>)> = Vec::new();
        for (id, note) in self.vault.notes.iter().enumerate() {
            if !base.includes(view, note) {
                continue;
            }
            let cells = columns
                .iter()
                .map(|column| self.base_cell(page, from, id, column, depth, diags))
                .collect();
            rows.push((id, cells));
        }
        let notes = &self.vault.notes;
        rows.sort_by(|(a, _), (b, _)| {
            for (property, descending) in &view.sort {
                let ordering = compare_values(
                    &sort_value(&notes[*a], property),
                    &sort_value(&notes[*b], property),
                );
                let ordering = if *descending {
                    ordering.reverse()
                } else {
                    ordering
                };
                if ordering.is_ne() {
                    return ordering;
                }
            }
            compare_values(&notes[*a].name, &notes[*b].name)
        });
        if let Some(limit) = view.limit {
            rows.truncate(limit);
        }

        let mut html = String::from("<div class=\"base\">\n<table>\n<thead>\n<tr>");
        for column in &columns {
            html.push_str(&format!("<th>{}</th>", escape_html(&base.header(column))));
        }
        html.push_str("</tr>\n</thead>\n<tbody>\n");
        for (_, cells) in rows {
            html.push_str("<tr>");
            for cell in cells {
                html.push_str(&format!("<td>{cell}</td>"));
            }
            html.push_str("</tr>\n");
        }
        html.push_str("</tbody>\n</table>\n</div>");
        html
    }

    fn base_cell(
        &self,
        page: &mut Page,
        from: NoteId,
        id: NoteId,
        column: &str,
        depth: usize,
        diags: &mut Diagnostics,
    ) -> String {
        let note = &self.vault.notes[id];
        match column_key(column) {
            "file.name" | "file.basename" => self.note_anchor(id, None, &note.name),
            "file.path" => escape_html(&note.path),
            "file.folder" => escape_html(&note.folder),
            "file.ext" => "md".to_owned(),
            "file.tags" | "tags" => note
                .tags
                .iter()
                .map(|t| tag_html(t))
                .collect::<Vec<_>>()
                .join(" "),
            key => note
                .property(key)
                .and_then(|value| self.format_value(page, from, key, value, depth, diags))
                .unwrap_or_default(),
        }
    }

    fn properties_table(
        &self,
        page: &mut Page,
        id: NoteId,
        diags: &mut Diagnostics,
    ) -> Option<String> {
        let note = &self.vault.notes[id];
        let mut rows = Vec::new();
        for (key, value) in &note.properties {
            // Formatted even when hidden, so links in hidden properties
            // still count as backlinks.
            let formatted = self.format_value(page, id, key, value, 0, diags);
            if self.hidden.iter().any(|glob| glob.matches(key)) {
                continue;
            }
            if let Some(html) = formatted {
                rows.push(format!(
                    "<tr><th>{}</th><td>{html}</td></tr>",
                    escape_html(key)
                ));
            }
        }
        (!rows.is_empty()).then(|| {
            format!(
                "<table class=\"properties\">\n<tbody>\n{}\n</tbody>\n</table>",
                rows.join("\n")
            )
        })
    }

    /// A property value as HTML: lists joined with `, `, booleans as ✓/✗,
    /// `[[links]]` resolved, URLs (and `domain`) linked, tags as chips.
    fn format_value(
        &self,
        page: &mut Page,
        from: NoteId,
        key: &str,
        value: &Pod,
        depth: usize,
        diags: &mut Diagnostics,
    ) -> Option<String> {
        if matches!(key, "tags" | "tag") {
            let tags: Vec<String> = property_strings(value)
                .iter()
                .flat_map(|s| {
                    s.split(|c: char| c == ',' || c.is_whitespace())
                        .map(|t| t.trim().trim_start_matches('#').to_owned())
                        .collect::<Vec<_>>()
                })
                .filter(|t| !t.is_empty())
                .map(|t| tag_html(&t))
                .collect();
            return (!tags.is_empty()).then(|| tags.join(" "));
        }
        match value {
            Pod::Null => None,
            Pod::String(s) if s.trim().is_empty() => None,
            Pod::String(s) => Some(self.format_string(page, from, key, s, depth, diags)),
            Pod::Integer(i) => Some(i.to_string()),
            Pod::Float(f) => Some(f.to_string()),
            Pod::Boolean(true) => {
                Some("<span class=\"prop-true\" title=\"true\">✓</span>".to_owned())
            }
            Pod::Boolean(false) => {
                Some("<span class=\"prop-false\" title=\"false\">✗</span>".to_owned())
            }
            Pod::Array(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .filter_map(|item| self.format_value(page, from, key, item, depth, diags))
                    .collect();
                (!parts.is_empty()).then(|| parts.join(", "))
            }
            Pod::Hash(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let parts: Vec<String> = keys
                    .into_iter()
                    .filter_map(|k| {
                        self.format_value(page, from, k, &map[k], depth, diags)
                            .map(|v| format!("{}: {v}", escape_html(k)))
                    })
                    .collect();
                (!parts.is_empty()).then(|| parts.join(", "))
            }
        }
    }

    fn format_string(
        &self,
        page: &mut Page,
        from: NoteId,
        key: &str,
        value: &str,
        depth: usize,
        diags: &mut Diagnostics,
    ) -> String {
        let value = value.trim();
        if value.contains("[[") {
            let mut out = String::new();
            let mut rest = value;
            while let Some(open) = rest.find("[[") {
                let Some(close) = rest[open..].find("]]") else {
                    break;
                };
                out.push_str(&escape_html(&rest[..open]));
                let inner = &rest[open + 2..open + close];
                let (target, alias) = match inner.split_once('|') {
                    Some((t, a)) => (t.trim(), Some(a.trim())),
                    None => (inner.trim(), None),
                };
                let display = alias
                    .filter(|a| !a.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| scan::display_for_target(target));
                out.push_str(&match self.resolve(page, from, target, depth, diags) {
                    Some(url) => format!(
                        "<a href=\"{}\">{}</a>",
                        escape_html(&url),
                        escape_html(&display)
                    ),
                    None if self.vault.resolve(from, target) == Resolution::Unresolved => {
                        broken_span(&display, target)
                    }
                    None => escape_html(&display),
                });
                rest = &rest[open + close + 2..];
            }
            out.push_str(&escape_html(rest));
            return out;
        }
        let is_url = value.starts_with("http://") || value.starts_with("https://");
        if is_url || (key == "domain" && !value.contains(char::is_whitespace)) {
            let href = if is_url {
                value.to_owned()
            } else {
                format!("https://{value}")
            };
            return format!(
                "<a href=\"{}\">{}</a>",
                escape_html(&href),
                escape_html(value)
            );
        }
        escape_html(value)
    }
}

/// Whether the text before an embed on its line leaves it alone on the
/// line, as a block: only blockquote markers, indentation and at most one
/// list marker.
fn is_block_prefix(before: &str) -> bool {
    let rest = before.trim_start_matches([' ', '\t', '>']);
    if rest.is_empty() {
        return true;
    }
    let marker_end = if rest.starts_with(['-', '*', '+']) {
        1
    } else {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 || !rest[digits..].starts_with(['.', ')']) {
            return false;
        }
        digits + 1
    };
    rest[marker_end..].starts_with([' ', '\t']) && rest[marker_end..].trim().is_empty()
}

/// From a heading to the next top-level heading of the same or a higher
/// level. `None` for headings inside containers.
fn section_range(note: &Note, heading_id: &str) -> Option<Range<usize>> {
    let index = note.headings.iter().position(|h| h.id == heading_id)?;
    let heading = &note.headings[index];
    if !heading.top_level {
        return None;
    }
    let end = note.headings[index + 1..]
        .iter()
        .find(|h| h.top_level && h.level <= heading.level)
        .map_or(note.body.len(), |h| h.range.start);
    Some(heading.range.start..end)
}

/// `<details>`/`<div>` with a title line and Markdown content.
fn embed_wrapper(tag: &str, class: &str, title: &str, inner: &str) -> String {
    let title_tag = if tag == "details" { "summary" } else { "div" };
    let mut out = format!(
        "<{tag} class=\"{class}\">\n<{title_tag} class=\"embed-title\">{title}</{title_tag}>\n"
    );
    let inner = inner.trim_matches('\n');
    if !inner.trim().is_empty() {
        out.push_str("<div class=\"embed-content\">\n\n");
        out.push_str(inner);
        out.push_str("\n\n</div>\n");
    }
    out.push_str(&format!("</{tag}>"));
    out
}

fn file_embed(file: &vault::VaultFile, link: &Link) -> String {
    let url = escape_html(&file.url);
    let ext = file.extension.as_str();
    if IMAGE_EXTENSIONS.contains(&ext) {
        // `![[img.png|200]]` and `![[img.png|200x100]]` set the size.
        let alias = link.alias.as_deref().unwrap_or_default().trim();
        let size = alias.split_once('x').map_or((alias, ""), |(w, h)| (w, h));
        let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        let (alt, size_attrs) = if numeric(size.0) && (size.1.is_empty() || numeric(size.1)) {
            let mut attrs = format!(" width=\"{}\"", size.0);
            if !size.1.is_empty() {
                attrs.push_str(&format!(" height=\"{}\"", size.1));
            }
            (file.name.as_str(), attrs)
        } else if alias.is_empty() {
            (file.name.as_str(), String::new())
        } else {
            (alias, String::new())
        };
        return format!(
            "<img src=\"{url}\" alt=\"{}\"{size_attrs} />",
            escape_html(alt)
        );
    }
    if AUDIO_EXTENSIONS.contains(&ext) {
        return format!("<audio controls src=\"{url}\"></audio>");
    }
    if VIDEO_EXTENSIONS.contains(&ext) {
        return format!("<video controls src=\"{url}\"></video>");
    }
    format!(
        "<p class=\"embed-file\"><a href=\"{url}\">{}</a></p>",
        escape_html(&link.display())
    )
}

fn broken_span(display: &str, target: &str) -> String {
    format!(
        "<span class=\"broken-link\" title=\"{}\">{}</span>",
        escape_html(&format!("Not found: {target}")),
        escape_html(display)
    )
}

fn broken_block(display: &str, target: &str) -> String {
    format!(
        "<p class=\"broken-embed\">{}</p>",
        broken_span(display, target)
    )
}

/// A ```` ```dataview ```` (or similar) block in a muted, closed box: only
/// a running Obsidian can evaluate it.
fn query_block(text: &str, range: Range<usize>, lang: &str) -> String {
    let before = &text[line_start(text, range.start)..range.start];
    let prefix = continuation_prefix(before);
    let original = &text[range.clone()];
    let trailing_newline = original.ends_with('\n');
    let original = original.strip_suffix('\n').unwrap_or(original);
    let blank = prefix.trim_end();
    let mut out = format!(
        "<details class=\"query\" data-lang=\"{lang}\">\n{prefix}<summary>{lang} query: dynamic content, not rendered</summary>\n{blank}\n{prefix}"
    );
    out.push_str(original);
    out.push_str(&format!("\n{blank}\n{prefix}</details>"));
    if trailing_newline {
        out.push_str(&format!("\n{blank}\n"));
    }
    out
}

fn sort_value(note: &Note, property: &str) -> String {
    match property {
        "file.name" | "file.basename" => note.name.clone(),
        "file.path" => note.path.clone(),
        "file.folder" => note.folder.clone(),
        key => note
            .property(key)
            .map(|v| property_strings(v).join(", "))
            .unwrap_or_default(),
    }
}

/// Numbers numerically, everything else case-insensitively.
fn compare_values(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a
            .to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_block_prefix() {
        assert!(is_block_prefix(""));
        assert!(is_block_prefix("> > "));
        assert!(is_block_prefix("   "));
        assert!(is_block_prefix("- "));
        assert!(is_block_prefix("> 1. "));
        assert!(!is_block_prefix("text "));
        assert!(!is_block_prefix("-x "));
    }

    #[test]
    fn test_query_block_is_wrapped() {
        let text = "```dataviewjs\nlet x = 1;\n```\nafter\n";
        let out = query_block(text, 0..text.find("after").unwrap(), "dataviewjs");
        assert!(out.starts_with("<details class=\"query\" data-lang=\"dataviewjs\">\n<summary>"));
        assert!(
            out.contains("\n\n```dataviewjs\nlet x = 1;\n```\n\n</details>\n\n"),
            "{out}"
        );
    }
}
