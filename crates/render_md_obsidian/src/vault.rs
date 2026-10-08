//! Reads the vault once: notes (front matter, comment-free body, headings,
//! block ids, tags), other files, folders and folder notes, the URL route
//! of every page, and the lookup tables link resolution uses.

use crate::comments::strip_comments;
use crate::diagnostics::Diagnostics;
use crate::error::Error;
use crate::glob::Glob;
use crate::scan::{self, Item};
use crate::slug::{self, IdRegistry};
use render_md::gray_matter::Pod;
use render_md::gray_matter::engine::{Engine, YAML};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub type NoteId = usize;
pub type FileId = usize;

/// Files that are never part of the site. Dot-files and dot-folders
/// (`.obsidian/`, `.git/`, `.trash/`) are always skipped as well.
pub const DEFAULT_EXCLUDES: &[&str] = &["*.blueprint"];

/// Names (without `.md`, any case) of a note at the vault root that is
/// served at `/` when no home note is given, after the site title.
pub const INDEX_NAMES: &[&str] = &["index", "home", "readme"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    Note(NoteId),
    File(FileId),
}

#[derive(Debug, Clone)]
pub struct Note {
    /// Vault-relative path, `/`-separated: `lab/servers/web.md`.
    pub path: String,
    /// File name without `.md`: `web`.
    pub name: String,
    /// Vault-relative folder, `""` for the vault root.
    pub folder: String,
    /// URL path of the note's page, with a trailing slash.
    pub route: String,
    /// Front matter properties in file order.
    pub properties: Vec<(String, Pod)>,
    /// The note without its front matter and with `%%` comments removed.
    pub body: String,
    pub headings: Vec<IndexedHeading>,
    pub block_ids: Vec<String>,
    /// Front matter and inline tags, without `#`.
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    /// A board of the kanban plugin (`kanban-plugin` property).
    pub kanban: bool,
}

impl Note {
    pub fn property(&self, key: &str) -> Option<&Pod> {
        self.properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, value)| value)
    }
}

#[derive(Debug, Clone)]
pub struct IndexedHeading {
    pub text: String,
    /// The id on the note's own page.
    pub id: String,
    /// The slug before de-duplication, used for fuzzy matching.
    pub slug: String,
    pub level: u8,
    /// Range in [`Note::body`].
    pub range: Range<usize>,
    pub top_level: bool,
}

/// A vault file that isn't a note: an image, a PDF, a `.base`, ...
#[derive(Debug, Clone)]
pub struct VaultFile {
    pub path: String,
    /// File name with its extension.
    pub name: String,
    pub folder: String,
    /// Lowercase extension without the dot.
    pub extension: String,
    /// Where the file is published if a note embeds or links it.
    pub url: String,
}

#[derive(Debug, Clone, Default)]
pub struct Folder {
    /// Vault-relative path, `""` for the vault root.
    pub path: String,
    pub name: String,
    /// The URL path derived from the folder's own path.
    pub route: String,
    /// The folder note (`a/b/b.md` for `a/b`).
    pub note: Option<NoteId>,
    pub subfolders: Vec<String>,
    /// Notes directly in the folder, without the folder note.
    pub notes: Vec<NoteId>,
}

pub struct ScanOptions<'a> {
    /// `--exclude` globs, on top of [`DEFAULT_EXCLUDES`].
    pub excludes: &'a [String],
    /// Vault-relative path of the note served at `/`. Without it, an index
    /// note at the vault root is used, see [`INDEX_NAMES`].
    pub home: Option<&'a str>,
    /// The site title: a root note with this name is the index note.
    pub title: &'a str,
    /// Directories never scanned (the output and staging directories, if
    /// they're inside the vault).
    pub skip_dirs: &'a [PathBuf],
}

#[derive(Debug, Default)]
pub struct Vault {
    pub root: PathBuf,
    pub notes: Vec<Note>,
    pub files: Vec<VaultFile>,
    pub folders: BTreeMap<String, Folder>,
    pub home: Option<NoteId>,
    /// Lowercase paths and file names of excluded files: links to them are
    /// plain text, not broken links.
    pub(crate) excluded: HashSet<String>,
    /// Lowercase vault path (notes also without `.md`) to targets.
    pub(crate) by_path: HashMap<String, Vec<Target>>,
    /// Note names, aliases and file names to targets.
    pub(crate) by_name: HashMap<String, Vec<Target>>,
    pub(crate) by_name_lower: HashMap<String, Vec<Target>>,
}

impl Vault {
    pub fn scan(
        root: &Path,
        options: &ScanOptions,
        diags: &mut Diagnostics,
    ) -> Result<Self, Error> {
        if !root.is_dir() {
            return Err(Error::not_a_directory(root));
        }
        let excludes: Vec<Glob> = DEFAULT_EXCLUDES
            .iter()
            .copied()
            .chain(options.excludes.iter().map(String::as_str))
            .map(Glob::new)
            .collect();
        let skip_dirs: Vec<PathBuf> = options
            .skip_dirs
            .iter()
            .filter_map(|dir| dir.canonicalize().ok())
            .collect();

        let mut vault = Self {
            root: root.to_owned(),
            ..Default::default()
        };
        vault.folders.insert(String::new(), Folder::default());

        let walker = WalkDir::new(root)
            .follow_links(true)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !(entry.file_name().to_string_lossy().starts_with('.')
                        || (entry.file_type().is_dir()
                            && entry
                                .path()
                                .canonicalize()
                                .is_ok_and(|p| skip_dirs.contains(&p))))
            });

        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    let path = err
                        .path()
                        .map_or_else(String::new, |p| relative_path(root, p));
                    diags.warn(&path, format!("skipped: {err}"));
                    continue;
                }
            };
            if entry.depth() == 0 {
                continue;
            }
            let rel = relative_path(root, entry.path());
            if excludes.iter().any(|glob| glob.matches(&rel)) {
                if entry.file_type().is_file() {
                    vault.excluded.insert(rel.to_lowercase());
                    vault.excluded.insert(file_name(&rel).to_lowercase());
                }
                continue;
            }
            if entry.file_type().is_dir() {
                vault.ensure_folder(&rel);
            } else if entry.file_type().is_file() {
                vault.ensure_folder(parent(&rel));
                if extension(&rel) == "md" {
                    let raw =
                        std::fs::read(entry.path()).map_err(|e| Error::read(e, entry.path()))?;
                    let note = read_note(&rel, &String::from_utf8_lossy(&raw), diags);
                    vault.notes.push(note);
                } else {
                    vault.files.push(VaultFile {
                        name: file_name(&rel).to_owned(),
                        folder: parent(&rel).to_owned(),
                        extension: extension(&rel),
                        url: String::new(),
                        path: rel,
                    });
                }
            }
        }

        vault.assign_folder_notes();
        vault.home = match options.home {
            Some(home) => Some(
                vault
                    .find_note_by_path(home)
                    .ok_or_else(|| Error::home_not_found(home))?,
            ),
            None => vault.find_index(options.title),
        };
        match vault.home {
            None => diags.warn(
                "",
                format!(
                    "no index note: / is a generated listing; add '{}.md' (or index.md) at the vault root, or pass --home",
                    options.title
                ),
            ),
            Some(id) => {
                let note = &vault.notes[id];
                if vault.folders[&note.folder].note == Some(id) {
                    diags.warn(
                        &note.path,
                        format!(
                            "the home note is the folder note of '{}/', so that folder has no page of its own (its URL redirects to /)",
                            note.folder
                        ),
                    );
                }
            }
        }
        vault.assign_routes()?;
        vault.build_lookup_tables();
        Ok(vault)
    }

    fn ensure_folder(&mut self, path: &str) {
        if self.folders.contains_key(path) {
            return;
        }
        let parent_path = parent(path).to_owned();
        self.ensure_folder(&parent_path);
        self.folders
            .get_mut(&parent_path)
            .expect("parent folder was just ensured")
            .subfolders
            .push(path.to_owned());
        self.folders.insert(
            path.to_owned(),
            Folder {
                path: path.to_owned(),
                name: file_name(path).to_owned(),
                ..Default::default()
            },
        );
    }

    /// `a/b/b.md` is the folder note of `a/b`; every other note is listed
    /// in its folder.
    fn assign_folder_notes(&mut self) {
        for (id, note) in self.notes.iter().enumerate() {
            let folder = self
                .folders
                .get_mut(&note.folder)
                .expect("every note's folder exists");
            if !note.folder.is_empty() && folder.name == note.name && folder.note.is_none() {
                folder.note = Some(id);
            } else {
                folder.notes.push(id);
            }
        }
        let notes = &self.notes;
        for folder in self.folders.values_mut() {
            folder
                .subfolders
                .sort_by_key(|path| file_name(path).to_lowercase());
            folder
                .notes
                .sort_by_key(|id| (notes[*id].name.to_lowercase(), notes[*id].name.clone()));
        }
    }

    fn assign_routes(&mut self) -> Result<(), Error> {
        let mut owners: HashMap<String, String> = HashMap::new();
        let mut claim = |route: &str, owner: &str| -> Result<(), Error> {
            let key = route.trim_end_matches('/').to_owned();
            if let Some(first) = owners.get(&key) {
                return Err(Error::route_collision(route, first.as_str(), owner));
            }
            owners.insert(key, owner.to_owned());
            Ok(())
        };

        for folder in self.folders.values_mut() {
            folder.route = folder_route(&folder.path);
        }
        for id in 0..self.notes.len() {
            let note = &self.notes[id];
            let folder = &self.folders[&note.folder];
            let route = if self.home == Some(id) {
                "/".to_owned()
            } else if folder.note == Some(id) {
                folder.route.clone()
            } else {
                format!("{}{}/", folder.route, slug::path_segment(&note.name))
            };
            claim(&route, &note.path)?;
            self.notes[id].route = route;
        }
        // Folders without a folder note get a generated listing page.
        for folder in self.folders.values() {
            if folder.note.is_none() && !(folder.path.is_empty() && self.home.is_some()) {
                claim(&folder.route, &format!("{}/", folder.path))?;
            }
        }
        for file in &mut self.files {
            file.url = format!(
                "{}{}",
                folder_route(&file.folder),
                slug::path_segment(&file.name)
            );
        }
        Ok(())
    }

    fn build_lookup_tables(&mut self) {
        let add = |map: &mut HashMap<String, Vec<Target>>, key: String, target: Target| {
            let entry = map.entry(key).or_default();
            if !entry.contains(&target) {
                entry.push(target);
            }
        };
        for (id, note) in self.notes.iter().enumerate() {
            let target = Target::Note(id);
            let path = note.path.to_lowercase();
            add(
                &mut self.by_path,
                path.trim_end_matches(".md").to_owned(),
                target,
            );
            add(&mut self.by_path, path, target);
            for name in std::iter::once(&note.name).chain(&note.aliases) {
                add(&mut self.by_name, name.clone(), target);
                add(&mut self.by_name_lower, name.to_lowercase(), target);
            }
        }
        for (id, file) in self.files.iter().enumerate() {
            let target = Target::File(id);
            add(&mut self.by_path, file.path.to_lowercase(), target);
            add(&mut self.by_name, file.name.clone(), target);
            add(&mut self.by_name_lower, file.name.to_lowercase(), target);
        }
    }

    /// The index note at the vault root: named like the site title, or one
    /// of [`INDEX_NAMES`].
    fn find_index(&self, title: &str) -> Option<NoteId> {
        let root: Vec<NoteId> = (0..self.notes.len())
            .filter(|id| self.notes[*id].folder.is_empty())
            .collect();
        std::iter::once(title)
            .chain(INDEX_NAMES.iter().copied())
            .find_map(|name| {
                root.iter()
                    .copied()
                    .find(|id| self.notes[*id].name.eq_ignore_ascii_case(name.trim()))
            })
    }

    /// Finds a note by its vault-relative path, `.md` optional, exact case
    /// first.
    pub fn find_note_by_path(&self, path: &str) -> Option<NoteId> {
        let path = path.trim().trim_start_matches("./").trim_start_matches('/');
        let with_ext = if path.to_lowercase().ends_with(".md") {
            path.to_owned()
        } else {
            format!("{path}.md")
        };
        self.notes
            .iter()
            .position(|n| n.path == with_ext)
            .or_else(|| {
                let lower = with_ext.to_lowercase();
                self.notes
                    .iter()
                    .position(|n| n.path.to_lowercase() == lower)
            })
    }

    /// The page a folder links to: its folder note, or its listing page.
    pub fn folder_link(&self, path: &str) -> String {
        let folder = &self.folders[path];
        match folder.note {
            Some(id) => self.notes[id].route.clone(),
            None if path.is_empty() && self.home.is_some() => "/".to_owned(),
            None => folder.route.clone(),
        }
    }

    /// Folders whose own route isn't served by any page because their
    /// folder note is the home note: these get a redirect to `/`.
    pub fn redirected_folders(&self) -> Vec<&Folder> {
        self.folders
            .values()
            .filter(|f| !f.path.is_empty() && f.note.is_some() && f.note == self.home)
            .collect()
    }

    /// Folders that need a generated listing page.
    pub fn listing_folders(&self) -> Vec<&Folder> {
        self.folders
            .values()
            .filter(|f| f.note.is_none() && !(f.path.is_empty() && self.home.is_some()))
            .collect()
    }

    /// All tags with the notes carrying them, keyed by lowercase tag.
    pub fn tag_index(&self) -> BTreeMap<String, (String, Vec<NoteId>)> {
        let mut index: BTreeMap<String, (String, Vec<NoteId>)> = BTreeMap::new();
        for (id, note) in self.notes.iter().enumerate() {
            for tag in &note.tags {
                let entry = index
                    .entry(tag.to_lowercase())
                    .or_insert_with(|| (tag.clone(), Vec::new()));
                entry.1.push(id);
            }
        }
        index
    }
}

/// URL path of a tag page.
pub fn tag_route(tag: &str) -> String {
    let segments: Vec<String> = tag
        .split('/')
        .filter(|s| !s.is_empty())
        .map(slug::path_segment)
        .collect();
    format!("/tags/{}/", segments.join("/"))
}

fn folder_route(path: &str) -> String {
    if path.is_empty() {
        return "/".to_owned();
    }
    let segments: Vec<String> = path.split('/').map(slug::path_segment).collect();
    format!("/{}/", segments.join("/"))
}

fn relative_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

pub(crate) fn file_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |i| &path[i + 1..])
}

fn extension(path: &str) -> String {
    let name = file_name(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// Splits off YAML front matter: the first line is `---`, and so is the
/// line closing it. Returns the YAML and the rest of the note.
pub(crate) fn split_front_matter(text: &str) -> (Option<&str>, &str) {
    let Some(first_end) = text.find('\n') else {
        return (None, text);
    };
    if text[..first_end].trim_end() != "---" {
        return (None, text);
    }
    let mut pos = first_end + 1;
    while pos <= text.len() {
        let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
        if text[pos..end].trim_end() == "---" {
            let body_start = (end + 1).min(text.len());
            return (Some(&text[first_end + 1..pos]), &text[body_start..]);
        }
        if end == text.len() {
            break;
        }
        pos = end + 1;
    }
    (None, text)
}

/// Top-level keys of a YAML mapping, in file order (the parsed `Pod` is a
/// `HashMap` and loses it).
fn top_level_keys(yaml: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for line in yaml.lines() {
        if line.trim().is_empty() || line.starts_with([' ', '\t', '-', '#']) {
            continue;
        }
        let Some((key, _)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches(|c| c == '"' || c == '\'');
        if !key.is_empty() && !keys.iter().any(|k| k == key) {
            keys.push(key.to_owned());
        }
    }
    keys
}

fn parse_properties(path: &str, yaml: &str, diags: &mut Diagnostics) -> Vec<(String, Pod)> {
    let mut hash = match YAML::parse(yaml) {
        Ok(Pod::Hash(hash)) => hash,
        Ok(Pod::Null) => return Vec::new(),
        Ok(_) => {
            diags.warn(path, "front matter is not a mapping, ignoring it");
            return Vec::new();
        }
        Err(err) => {
            diags.warn(path, format!("invalid front matter, ignoring it: {err}"));
            return Vec::new();
        }
    };
    let mut properties: Vec<(String, Pod)> = top_level_keys(yaml)
        .into_iter()
        .filter_map(|key| hash.remove(&key).map(|value| (key, value)))
        .collect();
    let mut rest: Vec<(String, Pod)> = hash.into_iter().collect();
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    properties.extend(rest);
    properties
}

/// All strings in a property value (a string, or a list of them).
pub(crate) fn property_strings(value: &Pod) -> Vec<String> {
    match value {
        Pod::String(s) => vec![s.clone()],
        Pod::Integer(i) => vec![i.to_string()],
        Pod::Float(f) => vec![f.to_string()],
        Pod::Boolean(b) => vec![b.to_string()],
        Pod::Array(items) => items.iter().flat_map(property_strings).collect(),
        Pod::Null | Pod::Hash(_) => Vec::new(),
    }
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    let lower = value.to_lowercase();
    if !value.is_empty() && !list.iter().any(|v| v.to_lowercase() == lower) {
        list.push(value.to_owned());
    }
}

/// Assigns ids to headings in document order, exactly as the transform
/// does for the note's own page.
pub(crate) fn heading_id(ids: &mut IdRegistry, prefix: &str, heading: &scan::Heading) -> String {
    match &heading.explicit_id {
        Some(id) => {
            ids.reserve(id);
            id.clone()
        }
        None => ids.assign(&format!("{prefix}{}", slug::heading_id(&heading.text))),
    }
}

fn read_note(path: &str, raw: &str, diags: &mut Diagnostics) -> Note {
    let text = raw.replace("\r\n", "\n");
    let (yaml, body) = split_front_matter(&text);
    let properties = yaml
        .map(|yaml| parse_properties(path, yaml, diags))
        .unwrap_or_default();
    let body = strip_comments(body);

    let mut tags = Vec::new();
    let mut aliases = Vec::new();
    for (key, value) in &properties {
        match key.as_str() {
            "tags" | "tag" => {
                for s in property_strings(value) {
                    for tag in s.split(|c: char| c == ',' || c.is_whitespace()) {
                        push_unique(&mut tags, tag.trim().trim_start_matches('#'));
                    }
                }
            }
            "aliases" | "alias" => {
                for alias in property_strings(value) {
                    push_unique(&mut aliases, alias.trim());
                }
            }
            _ => {}
        }
    }

    let mut ids = IdRegistry::default();
    let mut headings = Vec::new();
    let mut block_ids = Vec::new();
    for item in scan::scan(&body) {
        match item {
            Item::Heading(h) => headings.push(IndexedHeading {
                id: heading_id(&mut ids, "", &h),
                slug: slug::heading_id(&h.text),
                text: h.text,
                level: h.level,
                range: h.range,
                top_level: h.top_level,
            }),
            Item::BlockId { id, .. } => block_ids.push(id),
            Item::Tag { tag, .. } => push_unique(&mut tags, &tag),
            _ => {}
        }
    }

    let name = file_name(path);
    Note {
        path: path.to_owned(),
        name: name[..name.len() - 3].to_owned(),
        folder: parent(path).to_owned(),
        route: String::new(),
        kanban: properties.iter().any(|(k, _)| k == "kanban-plugin"),
        properties,
        body,
        headings,
        block_ids,
        tags,
        aliases,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_vault(files: &[&str], home: Option<&str>) -> (Vault, Diagnostics) {
        let dir = tempfile::tempdir().unwrap();
        for path in files {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, "").unwrap();
        }
        let mut diags = Diagnostics::default();
        let options = ScanOptions {
            excludes: &[],
            home,
            title: "infra",
            skip_dirs: &[],
        };
        let vault = Vault::scan(dir.path(), &options, &mut diags).unwrap();
        (vault, diags)
    }

    fn home_path(vault: &Vault) -> Option<&str> {
        vault.home.map(|id| vault.notes[id].path.as_str())
    }

    #[test]
    fn test_index_note_named_like_the_title_is_home() {
        let (vault, diags) = scan_vault(&["Infra.md", "index.md", "lab/lab.md"], None);
        assert_eq!(home_path(&vault), Some("Infra.md"));
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            vault.notes[vault.find_note_by_path("lab/lab.md").unwrap()].route,
            "/lab/"
        );
    }

    #[test]
    fn test_index_md_is_home_without_a_title_note() {
        let (vault, _) = scan_vault(&["README.md", "index.md"], None);
        assert_eq!(home_path(&vault), Some("index.md"));
    }

    #[test]
    fn test_missing_index_is_a_warning() {
        let (vault, diags) = scan_vault(&["lab/lab.md", "lab/index.md"], None);
        assert_eq!(home_path(&vault), None);
        let messages: Vec<String> = diags.iter().map(|d| d.to_string()).collect();
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].starts_with("warning: no index note"),
            "{messages:?}"
        );
        assert!(messages[0].contains("'infra.md'"));
    }

    #[test]
    fn test_folder_note_as_home_is_a_warning() {
        let (vault, diags) = scan_vault(&["lab/lab.md"], Some("lab/lab.md"));
        assert_eq!(home_path(&vault), Some("lab/lab.md"));
        let messages: Vec<String> = diags.iter().map(|d| d.to_string()).collect();
        assert!(
            messages[0].contains("folder note of 'lab/'"),
            "{messages:?}"
        );
    }

    #[test]
    fn test_split_front_matter() {
        assert_eq!(
            split_front_matter("---\na: 1\n---\nbody\n"),
            (Some("a: 1\n"), "body\n")
        );
        assert_eq!(
            split_front_matter("---\n\nkanban-plugin: board\n\n---\n\n## TODO"),
            (Some("\nkanban-plugin: board\n\n"), "\n## TODO")
        );
        assert_eq!(
            split_front_matter("# no matter\n---\n"),
            (None, "# no matter\n---\n")
        );
        assert_eq!(split_front_matter("---\nunclosed"), (None, "---\nunclosed"));
        assert_eq!(split_front_matter("---\na: 1\n---"), (Some("a: 1\n"), ""));
    }

    #[test]
    fn test_properties_keep_file_order_and_types() {
        let mut diags = Diagnostics::default();
        let note = read_note(
            "a/n.md",
            "---\nzeta: x\nip:\n  - 10.0.0.1\nnode: 7\nflag: true\ntags: [one, '#two']\n---\nbody #three\n",
            &mut diags,
        );
        let keys: Vec<&str> = note.properties.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["zeta", "ip", "node", "flag", "tags"]);
        assert_eq!(note.property("node"), Some(&Pod::Integer(7)));
        assert_eq!(note.tags, ["one", "two", "three"]);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_invalid_front_matter_is_a_warning() {
        let mut diags = Diagnostics::default();
        let note = read_note("n.md", "---\na: [unclosed\n---\nbody", &mut diags);
        assert!(note.properties.is_empty());
        assert_eq!(note.body, "body");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_headings_get_page_ids() {
        let mut diags = Diagnostics::default();
        let note = read_note("n.md", "# Notes\n## Notes\n## Auth (a / b)\n", &mut diags);
        let ids: Vec<&str> = note.headings.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["notes", "notes-1", "auth-a--b"]);
    }
}
