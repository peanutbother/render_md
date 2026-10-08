//! The subset of Obsidian Bases (`.base` files) that a static page can
//! show: `and`/`or`/`not` filters over `file.inFolder()`,
//! `file.hasProperty()` and `file.hasTag()`, and the first table view with
//! its column `order`, `sort` and `limit`.

use crate::vault::{Note, property_strings};
use render_md::gray_matter::Pod;
use render_md::gray_matter::engine::{Engine, YAML};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Not(Vec<Filter>),
    Call {
        negated: bool,
        function: Function,
        arg: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Function {
    InFolder,
    HasProperty,
    HasTag,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub name: Option<String>,
    pub order: Vec<String>,
    /// `(property, descending)`
    pub sort: Vec<(String, bool)>,
    pub limit: Option<usize>,
    pub filter: Option<Filter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Base {
    pub filter: Option<Filter>,
    /// Table views, in file order.
    pub views: Vec<View>,
    /// Column display names, keyed by property (`note.` prefix removed).
    pub display_names: HashMap<String, String>,
}

impl Base {
    pub fn parse(text: &str) -> Result<Self, String> {
        let root = match YAML::parse(text).map_err(|e| e.to_string())? {
            Pod::Hash(root) => root,
            Pod::Null => HashMap::new(),
            _ => return Err("a base must be a YAML mapping".to_owned()),
        };
        let filter = root.get("filters").map(parse_filter).transpose()?;

        let mut views = Vec::new();
        if let Some(Pod::Array(items)) = root.get("views") {
            for item in items {
                let Pod::Hash(view) = item else { continue };
                if view.get("type").and_then(as_str) != Some("table") {
                    continue;
                }
                views.push(View {
                    name: view.get("name").and_then(as_str).map(str::to_owned),
                    order: view.get("order").map(property_strings).unwrap_or_default(),
                    sort: match view.get("sort") {
                        Some(Pod::Array(sorts)) => sorts.iter().filter_map(parse_sort).collect(),
                        _ => Vec::new(),
                    },
                    limit: match view.get("limit") {
                        Some(Pod::Integer(n)) if *n > 0 => Some(*n as usize),
                        _ => None,
                    },
                    filter: view.get("filters").map(parse_filter).transpose()?,
                });
            }
        }

        let mut display_names = HashMap::new();
        if let Some(Pod::Hash(props)) = root.get("properties") {
            for (key, config) in props {
                if let Pod::Hash(config) = config
                    && let Some(name) = config.get("displayName").and_then(as_str)
                {
                    display_names.insert(column_key(key).to_owned(), name.to_owned());
                }
            }
        }

        Ok(Self {
            filter,
            views,
            display_names,
        })
    }

    /// The view to show: the one named `name`, or the first table view.
    pub fn view(&self, name: Option<&str>) -> Option<&View> {
        match name {
            Some(name) => self.views.iter().find(|v| v.name.as_deref() == Some(name)),
            None => self.views.first(),
        }
    }

    pub fn includes(&self, view: &View, note: &Note) -> bool {
        self.filter.as_ref().is_none_or(|f| f.matches(note))
            && view.filter.as_ref().is_none_or(|f| f.matches(note))
    }

    pub fn header(&self, column: &str) -> String {
        let key = column_key(column);
        self.display_names.get(key).cloned().unwrap_or_else(|| {
            match key {
                "file.name" | "file.basename" => "name",
                other => other,
            }
            .to_owned()
        })
    }
}

/// `note.service` and `service` are the same column.
pub fn column_key(column: &str) -> &str {
    column.strip_prefix("note.").unwrap_or(column)
}

impl Filter {
    pub fn matches(&self, note: &Note) -> bool {
        match self {
            Filter::And(all) => all.iter().all(|f| f.matches(note)),
            Filter::Or(any) => any.iter().any(|f| f.matches(note)),
            Filter::Not(none) => !none.iter().any(|f| f.matches(note)),
            Filter::Call {
                negated,
                function,
                arg,
            } => {
                let result = match function {
                    Function::InFolder => {
                        let folder = arg.trim_matches('/');
                        folder.is_empty()
                            || note.folder == folder
                            || note.folder.starts_with(&format!("{folder}/"))
                    }
                    Function::HasProperty => {
                        note.properties.iter().any(|(k, _)| k == column_key(arg))
                    }
                    Function::HasTag => {
                        let wanted = arg.trim_start_matches('#').to_lowercase();
                        note.tags.iter().any(|t| {
                            let t = t.to_lowercase();
                            t == wanted || t.starts_with(&format!("{wanted}/"))
                        })
                    }
                };
                result != *negated
            }
        }
    }
}

fn as_str(pod: &Pod) -> Option<&str> {
    match pod {
        Pod::String(s) => Some(s),
        _ => None,
    }
}

fn parse_sort(pod: &Pod) -> Option<(String, bool)> {
    let Pod::Hash(sort) = pod else { return None };
    let property = sort.get("property").and_then(as_str)?;
    let descending = sort
        .get("direction")
        .and_then(as_str)
        .is_some_and(|d| d.eq_ignore_ascii_case("desc"));
    Some((column_key(property).to_owned(), descending))
}

fn parse_filter(pod: &Pod) -> Result<Filter, String> {
    match pod {
        Pod::String(expr) => parse_call(expr),
        Pod::Hash(map) if map.len() == 1 => {
            let (op, items) = map.iter().next().expect("one entry");
            let items = match items {
                Pod::Array(items) => items.iter().map(parse_filter).collect::<Result<_, _>>()?,
                other => vec![parse_filter(other)?],
            };
            match op.as_str() {
                "and" => Ok(Filter::And(items)),
                "or" => Ok(Filter::Or(items)),
                "not" => Ok(Filter::Not(items)),
                other => Err(format!("unsupported filter operator '{other}'")),
            }
        }
        _ => Err("unsupported filter".to_owned()),
    }
}

/// Parses `file.inFolder("path")`, optionally negated with `!`.
fn parse_call(expr: &str) -> Result<Filter, String> {
    let unsupported = || format!("unsupported filter expression '{expr}'");
    let trimmed = expr.trim();
    let (negated, call) = match trimmed.strip_prefix('!') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, trimmed),
    };
    let open = call.find('(').ok_or_else(unsupported)?;
    let args = call[open + 1..]
        .trim_end()
        .strip_suffix(')')
        .ok_or_else(unsupported)?
        .trim();
    let function = match call[..open].trim() {
        "file.inFolder" => Function::InFolder,
        "file.hasProperty" => Function::HasProperty,
        "file.hasTag" => Function::HasTag,
        _ => return Err(unsupported()),
    };
    let quote = args
        .chars()
        .next()
        .filter(|c| *c == '"' || *c == '\'')
        .ok_or_else(unsupported)?;
    let arg = args[1..].strip_suffix(quote).ok_or_else(unsupported)?;
    if arg.contains(quote) {
        return Err(unsupported());
    }
    Ok(Filter::Call {
        negated,
        function,
        arg: arg.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"filters:
  and:
    - file.inFolder("lab/servers")
    - file.hasProperty("blueprint")
    - '!file.hasTag("archived")'
views:
  - type: cards
    name: Cards
  - type: table
    name: Servers
    order:
      - file.name
      - note.service
      - ip
    sort:
      - property: note.node
        direction: DESC
properties:
  note.service:
    displayName: Service
"#;

    fn note(folder: &str, properties: &[&str], tags: &[&str]) -> Note {
        Note {
            path: format!("{folder}/n.md"),
            name: "n".to_owned(),
            folder: folder.to_owned(),
            route: String::new(),
            properties: properties
                .iter()
                .map(|k| (k.to_string(), Pod::Boolean(true)))
                .collect(),
            body: String::new(),
            headings: Vec::new(),
            block_ids: Vec::new(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            aliases: Vec::new(),
            kanban: false,
        }
    }

    #[test]
    fn test_parse_base() {
        let base = Base::parse(BASE).unwrap();
        let view = base.view(None).unwrap();
        assert_eq!(view.name.as_deref(), Some("Servers"));
        assert_eq!(view.order, ["file.name", "note.service", "ip"]);
        assert_eq!(view.sort, [("node".to_owned(), true)]);
        assert_eq!(base.header("note.service"), "Service");
        assert_eq!(base.header("file.name"), "name");
        assert_eq!(base.header("ip"), "ip");
    }

    #[test]
    fn test_filters() {
        let base = Base::parse(BASE).unwrap();
        let view = base.view(None).unwrap();
        assert!(base.includes(view, &note("lab/servers", &["blueprint"], &[])));
        assert!(base.includes(view, &note("lab/servers/logs", &["blueprint"], &[])));
        assert!(!base.includes(view, &note("lab/serversX", &["blueprint"], &[])));
        assert!(!base.includes(view, &note("lab/servers", &[], &[])));
        assert!(!base.includes(view, &note("lab/servers", &["blueprint"], &["archived"])));
    }

    #[test]
    fn test_unsupported_expression_is_an_error() {
        let err = Base::parse("filters: 'file.mtime > now()'\n").unwrap_err();
        assert!(err.contains("file.mtime > now()"), "{err}");
    }
}
