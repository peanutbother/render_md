//! The offset pass: one `pulldown_cmark` run over a note's text that finds
//! every Obsidian construct with its exact source span. Code blocks and
//! inline code are skipped by the parser itself, so `[[ $x -gt 0 ]]` in a
//! shell snippet is never mistaken for a wikilink.
//!
//! The same scan feeds the vault index (headings, block ids, tags) and the
//! note transform, so heading ids computed for link targets always match
//! the ids written into the page.

use crate::text::{line_end, line_start, parser_options};
use pulldown_cmark::{CodeBlockKind, Event, LinkType, Parser, Tag, TagEnd};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// `[[target|alias]]`; `range` covers the brackets.
    WikiLink(Link),
    /// `![[target|alias]]`; `range` covers the `!` and the brackets.
    Embed(Link),
    Heading(Heading),
    /// A block id (`^abc` at the end of a paragraph or list item); `range`
    /// covers `^abc` and the whitespace before it.
    BlockId {
        range: Range<usize>,
        id: String,
    },
    /// An inline `#tag`; `range` covers the `#` and the tag.
    Tag {
        range: Range<usize>,
        tag: String,
    },
    /// A ```` ```dataview ````, ```` ```dataviewjs ```` or ```` ```tasks ````
    /// query block, which only a running Obsidian can evaluate.
    Query {
        range: Range<usize>,
        lang: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub range: Range<usize>,
    /// The link target, e.g. `note#Heading`, with the backslash of an
    /// escaped table pipe (`[[a\|b]]`) removed.
    pub target: String,
    pub alias: Option<String>,
}

impl Link {
    /// What Obsidian shows for this link: the alias if there is one,
    /// otherwise `note > heading`.
    pub fn display(&self) -> String {
        match &self.alias {
            Some(alias) if !alias.trim().is_empty() => alias.trim().to_owned(),
            _ => display_for_target(&self.target),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub range: Range<usize>,
    pub level: u8,
    /// The heading's plain text (inline markup removed, wikilinks replaced
    /// by their display text).
    pub text: String,
    /// An id the author wrote explicitly (`# Title {#id}`).
    pub explicit_id: Option<String>,
    /// Where to add the generated id.
    pub insert: IdInsert,
    /// Not inside a blockquote, list or footnote; only these headings
    /// delimit sections for `![[note#Heading]]`.
    pub top_level: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IdInsert {
    /// Append ` {#id}` at this offset (the end of the heading text line).
    Append(usize),
    /// Insert ` #id` at this offset, just before the `}` of an existing
    /// attribute block (`# Title {.class}`).
    IntoBraces(usize),
    /// The heading already has an explicit id.
    Keep,
}

/// `[[a#b]]` shows as `a > b`, `[[#b]]` as `b`, `[[path/a]]` as `a`.
pub fn display_for_target(target: &str) -> String {
    let mut parts = target.split('#').map(str::trim);
    let path = parts.next().unwrap_or_default();
    let name = path.rsplit('/').next().unwrap_or(path);
    let name = name.strip_suffix(".md").unwrap_or(name);
    let mut shown: Vec<&str> = Vec::new();
    if !name.is_empty() {
        shown.push(name);
    }
    shown.extend(parts.filter(|p| !p.is_empty()));
    shown.join(" > ")
}

/// Splits the inside of `[[…]]` into target and alias. In tables the pipe
/// has to be escaped (`[[a\|b]]`), and the parser leaves the backslash on
/// the target.
fn split_wikilink(inner: &str) -> (String, Option<String>) {
    match inner.find('|') {
        Some(i) => {
            let target = inner[..i].strip_suffix('\\').unwrap_or(&inner[..i]);
            (target.trim().to_owned(), Some(inner[i + 1..].to_owned()))
        }
        None => (inner.trim().to_owned(), None),
    }
}

fn wikilink_at(text: &str, range: Range<usize>, embed: bool) -> Link {
    let open = if embed { 3 } else { 2 };
    let inner = text
        .get(range.start + open..range.end.saturating_sub(2))
        .unwrap_or_default();
    let (target, alias) = split_wikilink(inner);
    Link {
        range,
        target,
        alias,
    }
}

/// Scans `text` and returns its items in document order.
pub fn scan(text: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut in_code = false;
    let mut link_depth = 0usize;
    let mut containers = 0usize;
    // Inside a wikilink or embed: its events are only its display text.
    let mut in_wikilink = false;
    let mut heading: Option<(Heading, bool)> = None;

    for (event, range) in Parser::new_ext(text, parser_options()).into_offset_iter() {
        if in_wikilink {
            in_wikilink = !matches!(event, Event::End(TagEnd::Link | TagEnd::Image));
            continue;
        }
        match event {
            Event::Start(Tag::Link {
                link_type: LinkType::WikiLink { .. },
                ..
            }) => {
                let link = wikilink_at(text, range.clone(), false);
                if let Some((h, _)) = heading.as_mut() {
                    h.text.push_str(&link.display());
                }
                items.push(Item::WikiLink(link));
                in_wikilink = true;
            }
            Event::Start(Tag::Image {
                link_type: LinkType::WikiLink { .. },
                ..
            }) => {
                items.push(Item::Embed(wikilink_at(text, range.clone(), true)));
                in_wikilink = true;
            }
            Event::Start(Tag::Link { .. }) => link_depth += 1,
            Event::End(TagEnd::Link) => link_depth = link_depth.saturating_sub(1),
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code = true;
                if let CodeBlockKind::Fenced(info) = kind {
                    let lang = info.split_whitespace().next().unwrap_or_default();
                    if matches!(lang, "dataview" | "dataviewjs" | "tasks") {
                        items.push(Item::Query {
                            range: range.clone(),
                            lang: lang.to_owned(),
                        });
                    }
                }
            }
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Start(
                Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_),
            ) => containers += 1,
            Event::End(
                TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition,
            ) => containers = containers.saturating_sub(1),
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            }) => {
                let has_attr_block = id.is_some() || !classes.is_empty() || !attrs.is_empty();
                heading = Some((
                    Heading {
                        range: range.clone(),
                        level: level as u8,
                        text: String::new(),
                        explicit_id: id.map(|id| id.to_string()),
                        insert: IdInsert::Keep,
                        top_level: containers == 0,
                    },
                    has_attr_block,
                ));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((mut h, has_attr_block)) = heading.take() {
                    h.text = h.text.trim().to_owned();
                    h.insert = id_insert(text, &h, has_attr_block);
                    items.push(Item::Heading(h));
                }
            }
            Event::Text(t) if !in_code => {
                if let Some((h, _)) = heading.as_mut() {
                    h.text.push_str(&t);
                }
                if link_depth == 0 {
                    scan_tags(text, range.clone(), &mut items);
                    scan_block_id(text, range, &mut items);
                }
            }
            Event::Code(code) => {
                if let Some((h, _)) = heading.as_mut() {
                    h.text.push_str(&code);
                }
            }
            _ => {}
        }
    }

    // Tags and block ids are found while walking text events, headings at
    // their end: restore document order.
    items.sort_by_key(item_start);
    items
}

fn item_start(item: &Item) -> usize {
    match item {
        Item::WikiLink(l) | Item::Embed(l) => l.range.start,
        Item::Heading(h) => h.range.start,
        Item::BlockId { range, .. } | Item::Tag { range, .. } | Item::Query { range, .. } => {
            range.start
        }
    }
}

/// Where a generated id goes, see [`IdInsert`].
fn id_insert(text: &str, heading: &Heading, has_attr_block: bool) -> IdInsert {
    if heading.explicit_id.is_some() {
        return IdInsert::Keep;
    }
    let is_atx = text[heading.range.start..].trim_start().starts_with('#');
    let text_line_end = if is_atx {
        line_end(text, heading.range.start)
    } else {
        // Setext: the id goes on the last text line, before the underline.
        let end = heading.range.end.min(text.len());
        let end = if text[..end].ends_with('\n') {
            end - 1
        } else {
            end
        };
        line_start(text, end).saturating_sub(1)
    };
    let line = &text[..text_line_end];
    let trimmed_end = line.trim_end().len();
    if has_attr_block && line.trim_end().ends_with('}') {
        IdInsert::IntoBraces(trimmed_end - 1)
    } else {
        IdInsert::Append(trimmed_end)
    }
}

/// Finds `#tags` in a text event: a `#` at the start of the text or after
/// whitespace, followed by letters, digits, `_`, `-` or `/`, with at least
/// one non-digit (`#1` is not a tag).
fn scan_tags(text: &str, range: Range<usize>, items: &mut Vec<Item>) {
    let slice = &text[range.clone()];
    for (i, _) in slice.match_indices('#') {
        let pos = range.start + i;
        let preceded_ok = text[..pos]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        if !preceded_ok {
            continue;
        }
        let tag: String = text[pos + 1..range.end]
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
            .collect();
        let tag = tag.trim_end_matches('/');
        if tag.is_empty() || tag.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        items.push(Item::Tag {
            range: pos..pos + 1 + tag.len(),
            tag: tag.to_owned(),
        });
    }
}

/// Finds a block id (`^abc`) ending a text event that is the last thing on
/// its line.
fn scan_block_id(text: &str, range: Range<usize>, items: &mut Vec<Item>) {
    let rest_of_line = &text[range.end..line_end(text, range.end)];
    if !rest_of_line.trim().is_empty() {
        return;
    }
    let slice = text[range.clone()].trim_end();
    let Some(caret) = slice.rfind('^') else {
        return;
    };
    let id = &slice[caret + 1..];
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return;
    }
    let before = &slice[..caret];
    let at_line_start = before.is_empty() && line_start(text, range.start) == range.start;
    let ws = before.len() - before.trim_end().len();
    if ws == 0 && !at_line_start {
        return;
    }
    let start = range.start + caret - ws;
    items.push(Item::BlockId {
        range: start..range.start + slice.len(),
        id: id.to_owned(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headings(text: &str) -> Vec<Heading> {
        scan(text)
            .into_iter()
            .filter_map(|i| match i {
                Item::Heading(h) => Some(h),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_wikilink_with_escaped_pipe_in_table() {
        let text = "| a |\n|---|\n| [[auth#Login (sso / proxy)\\|Login]] |\n";
        let items = scan(text);
        let Some(Item::WikiLink(link)) = items.first() else {
            panic!("{items:?}");
        };
        assert_eq!(link.target, "auth#Login (sso / proxy)");
        assert_eq!(link.alias.as_deref(), Some("Login"));
        assert_eq!(
            &text[link.range.clone()],
            "[[auth#Login (sso / proxy)\\|Login]]"
        );
    }

    #[test]
    fn test_embed_and_display_text() {
        let items = scan("![[note#Part]] and [[a/b#c#d]] and [[#self]]");
        let [
            Item::Embed(embed),
            Item::WikiLink(nested),
            Item::WikiLink(own),
        ] = &items[..]
        else {
            panic!("{items:?}");
        };
        assert_eq!(embed.target, "note#Part");
        assert_eq!(embed.display(), "note > Part");
        assert_eq!(nested.display(), "b > c > d");
        assert_eq!(own.display(), "self");
    }

    #[test]
    fn test_code_is_never_scanned() {
        let text = "`[[a]]` and\n\n```sh\n[[ $x -gt 0 ]] #notag\n```\n";
        assert!(scan(text).is_empty(), "{:?}", scan(text));
    }

    #[test]
    fn test_heading_text_and_id_insert_points() {
        let text = "# Title ##\n\n## With `code` and [[x|alias]]\n\nSetext\n---\n\n### Kept {#mine}\n\n#### Classy {.c}\n";
        let hs = headings(text);
        assert_eq!(hs.len(), 5);
        assert_eq!(hs[0].text, "Title");
        assert_eq!(hs[0].insert, IdInsert::Append(10));
        assert_eq!(hs[1].text, "With code and alias");
        assert_eq!(hs[2].text, "Setext");
        assert_eq!(
            hs[2].insert,
            IdInsert::Append(text.find("Setext").unwrap() + 6)
        );
        assert_eq!(hs[3].explicit_id.as_deref(), Some("mine"));
        assert_eq!(hs[3].insert, IdInsert::Keep);
        let IdInsert::IntoBraces(pos) = hs[4].insert else {
            panic!("{:?}", hs[4]);
        };
        assert_eq!(&text[pos..pos + 1], "}");
    }

    #[test]
    fn test_headings_in_containers_are_not_top_level() {
        let hs = headings("# Top\n\n> ## Quoted\n\n- ## Listed\n");
        let top: Vec<bool> = hs.iter().map(|h| h.top_level).collect();
        assert_eq!(top, [true, false, false]);
    }

    #[test]
    fn test_tags() {
        let items = scan("#fix at start, a #tag/sub and #1 or a#b or [link #x](u)");
        let tags: Vec<String> = items
            .iter()
            .filter_map(|i| match i {
                Item::Tag { tag, .. } => Some(tag.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tags, ["fix", "tag/sub"]);
    }

    #[test]
    fn test_block_ids() {
        let text = "A paragraph ^abc-1\n\n- item ^def\n\nnot^an-id\n\n^own\n";
        let ids: Vec<(String, &str)> = scan(text)
            .into_iter()
            .filter_map(|i| match i {
                Item::BlockId { range, id } => Some((id, &text[range])),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            [
                ("abc-1".to_owned(), " ^abc-1"),
                ("def".to_owned(), " ^def"),
                ("own".to_owned(), "^own"),
            ]
        );
    }

    #[test]
    fn test_query_blocks() {
        let items = scan("```dataviewjs\ndv.paragraph(1)\n```\n");
        assert!(matches!(&items[..], [Item::Query { lang, .. }] if lang == "dataviewjs"));
    }
}
