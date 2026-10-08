//! Assembles the staged render_md site (steps 6 and 7): one
//! `src/<route>/index.md` per note, folder listing and tag pages, the
//! sidebar partial and the theme. The staged tree is an ordinary render_md
//! source tree, so `compile_md` or the CGI binary could serve it as well.

use crate::diagnostics::Diagnostics;
use crate::error::Error;
use crate::text::escape_html;
use crate::theme;
use crate::transform::{EmbedMode, TransformedNote, Transformer, tag_html};
use crate::vault::{FileId, Folder, NoteId, ScanOptions, Vault, tag_route};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Theme files and partials live here inside the staged `src/`. Slugs never
/// contain `@`, so no note can collide with it.
pub const THEME_DIR: &str = "@theme";

#[derive(Debug, Clone)]
pub struct SiteOptions {
    pub vault: PathBuf,
    /// Vault-relative path of the note served at `/`.
    pub home: Option<String>,
    pub excludes: Vec<String>,
    /// Globs of properties hidden from the properties table, on top of
    /// [`crate::transform::DEFAULT_HIDDEN_PROPERTIES`].
    pub hidden_properties: Vec<String>,
    pub embeds: EmbedMode,
    pub max_embed_depth: usize,
    /// Site title; defaults to the vault folder's name.
    pub title: Option<String>,
    /// Title of the page at `/`; defaults to the home note's name.
    pub home_title: Option<String>,
    /// A directory whose `template.html`, `styles/` and `static/` override
    /// the built-in theme.
    pub theme: Option<PathBuf>,
    /// Directories inside the vault that must not be scanned.
    pub skip_dirs: Vec<PathBuf>,
}

/// What [`stage`] produced, for [`crate::render`].
#[derive(Debug, Default)]
pub struct Staged {
    pub dir: PathBuf,
    pub title: String,
    pub notes: usize,
    pub generated_pages: usize,
    /// `(route, target)` redirect pages to write into the output.
    pub redirects: Vec<(String, String)>,
    /// `(absolute source, url)` of vault files used by pages.
    pub files: Vec<(PathBuf, String)>,
    /// The theme's `static/` directory, if overridden.
    pub static_override: Option<PathBuf>,
}

/// Reads the vault and writes the staged site into `stage_dir`.
pub fn stage(
    options: &SiteOptions,
    stage_dir: &Path,
    diags: &mut Diagnostics,
) -> Result<Staged, Error> {
    let vault = Vault::scan(
        &options.vault,
        &ScanOptions {
            excludes: &options.excludes,
            home: options.home.as_deref(),
            skip_dirs: &options.skip_dirs,
        },
        diags,
    )?;
    let title = options.title.clone().unwrap_or_else(|| {
        options
            .vault
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Vault".to_owned())
    });

    let tags = vault.tag_index();
    if !tags.is_empty()
        && let Some(note) = vault.notes.iter().find(|n| n.route.starts_with("/tags/"))
    {
        return Err(Error::route_collision(
            note.route.as_str(),
            "the tag pages",
            note.path.as_str(),
        ));
    }

    let transformer = Transformer::new(
        &vault,
        options.embeds,
        options.max_embed_depth,
        &options.hidden_properties,
    );
    let transformed: Vec<TransformedNote> = (0..vault.notes.len())
        .map(|id| transformer.note(id, diags))
        .collect();

    let mut backlinks: BTreeMap<NoteId, BTreeSet<NoteId>> = BTreeMap::new();
    let mut used_files: BTreeSet<FileId> = BTreeSet::new();
    for (from, note) in transformed.iter().enumerate() {
        for to in &note.links {
            backlinks.entry(*to).or_default().insert(from);
        }
        used_files.extend(&note.files);
    }

    let src = stage_dir.join("src");
    let site = Site {
        vault: &vault,
        title: &title,
        home_title: options.home_title.as_deref(),
    };
    for (id, note) in transformed.iter().enumerate() {
        let page = site.note_page(id, note, backlinks.get(&id));
        write(&page_path(&src, &vault.notes[id].route), &page)?;
    }
    let listings = vault.listing_folders();
    for folder in &listings {
        write(&page_path(&src, &folder.route), &site.listing_page(folder))?;
    }
    if !tags.is_empty() {
        write(&page_path(&src, "/tags/"), &site.tags_index(&tags))?;
        for (tag, notes) in tags.values() {
            write(
                &page_path(&src, &tag_route(tag)),
                &site.tag_page(tag, notes),
            )?;
        }
    }

    let theme_dir = src.join(THEME_DIR);
    write(&theme_dir.join("nav.html"), &escape_directives(&site.nav()))?;
    theme::write_theme(&theme_dir, options.theme.as_deref())?;

    let page_routes: BTreeSet<String> = vault
        .notes
        .iter()
        .map(|n| n.route.clone())
        .chain(listings.iter().map(|f| f.route.clone()))
        .collect();
    let redirects = vault
        .redirected_folders()
        .into_iter()
        .filter(|f| !page_routes.contains(&f.route))
        .map(|f| (f.route.clone(), "/".to_owned()))
        .collect();
    let files = used_files
        .into_iter()
        .map(|id| {
            let file = &vault.files[id];
            (vault.root.join(&file.path), file.url.clone())
        })
        .collect();

    Ok(Staged {
        dir: stage_dir.to_owned(),
        title,
        notes: vault.notes.len(),
        generated_pages: listings.len() + if tags.is_empty() { 0 } else { tags.len() + 1 },
        redirects,
        files,
        static_override: options
            .theme
            .as_ref()
            .map(|t| t.join("static"))
            .filter(|p| p.is_dir()),
    })
}

struct Site<'a> {
    vault: &'a Vault,
    title: &'a str,
    home_title: Option<&'a str>,
}

impl Site<'_> {
    fn note_page(
        &self,
        id: NoteId,
        note: &TransformedNote,
        backlinks: Option<&BTreeSet<NoteId>>,
    ) -> String {
        let n = &self.vault.notes[id];
        let mut crumbs: Vec<(String, String)> = Vec::new();
        if self.vault.home != Some(id) {
            let mut folder = n.folder.as_str();
            let own_folder_note = self.vault.folders[folder].note == Some(id);
            let mut chain = Vec::new();
            while !folder.is_empty() {
                chain.push(folder);
                folder = crate::vault::parent(folder);
            }
            chain.reverse();
            if own_folder_note {
                chain.pop();
            }
            crumbs = chain
                .into_iter()
                .map(|f| {
                    (
                        self.vault.folders[f].name.clone(),
                        self.vault.folder_link(f),
                    )
                })
                .collect();
        }

        let is_home = self.vault.home == Some(id);
        let name = match self.home_title {
            Some(title) if is_home => title,
            _ => n.name.as_str(),
        };
        let mut body = self.header(name, &crumbs, !is_home);
        if let Some(properties) = &note.properties {
            body.push_str(properties);
            body.push_str("\n\n");
        }
        body.push_str(note.body.trim_matches('\n'));
        body.push_str("\n\n");
        if let Some(sources) = backlinks.filter(|s| !s.is_empty()) {
            body.push_str("<section class=\"backlinks\">\n<div class=\"backlinks-title\">Linked mentions</div>\n<ul>\n");
            for source in sources {
                let s = &self.vault.notes[*source];
                body.push_str(&format!(
                    "<li><a href=\"{}\">{}</a> <span class=\"backlink-folder\">{}</span></li>\n",
                    escape_html(&s.route),
                    escape_html(&s.name),
                    escape_html(&s.folder)
                ));
            }
            body.push_str("</ul>\n</section>\n");
        }
        if is_home {
            self.home_page(name, &body)
        } else {
            self.page(name, &body)
        }
    }

    fn listing_page(&self, folder: &Folder) -> String {
        let name = if folder.path.is_empty() {
            self.title.to_owned()
        } else {
            folder.name.clone()
        };
        let mut crumbs = Vec::new();
        let mut parent = crate::vault::parent(&folder.path);
        while !folder.path.is_empty() && !parent.is_empty() {
            crumbs.push((
                self.vault.folders[parent].name.clone(),
                self.vault.folder_link(parent),
            ));
            parent = crate::vault::parent(parent);
        }
        crumbs.reverse();
        let mut body = self.header(&name, &crumbs, !folder.path.is_empty());
        body.push_str("<ul class=\"listing\">\n");
        for sub in &folder.subfolders {
            body.push_str(&format!(
                "<li class=\"listing-folder\"><a href=\"{}\">{}/</a></li>\n",
                escape_html(&self.vault.folder_link(sub)),
                escape_html(&self.vault.folders[sub].name)
            ));
        }
        for id in &folder.notes {
            let n = &self.vault.notes[*id];
            body.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>\n",
                escape_html(&n.route),
                escape_html(&n.name)
            ));
        }
        body.push_str("</ul>\n");
        self.page(&name, &body)
    }

    fn tags_index(&self, tags: &BTreeMap<String, (String, Vec<NoteId>)>) -> String {
        let mut body = self.header("Tags", &[], true);
        body.push_str("<ul class=\"listing tag-listing\">\n");
        for (tag, notes) in tags.values() {
            body.push_str(&format!(
                "<li>{} <span class=\"tag-count\">{}</span></li>\n",
                tag_html(tag),
                notes.len()
            ));
        }
        body.push_str("</ul>\n");
        self.page("Tags", &body)
    }

    fn tag_page(&self, tag: &str, notes: &[NoteId]) -> String {
        let name = format!("#{tag}");
        let mut body = self.header(&name, &[("Tags".to_owned(), "/tags/".to_owned())], true);
        body.push_str("<ul class=\"listing\">\n");
        let mut sorted: Vec<&NoteId> = notes.iter().collect();
        sorted.sort_by_key(|id| self.vault.notes[**id].name.to_lowercase());
        for id in sorted {
            let n = &self.vault.notes[*id];
            body.push_str(&format!(
                "<li><a href=\"{}\">{}</a> <span class=\"backlink-folder\">{}</span></li>\n",
                escape_html(&n.route),
                escape_html(&n.name),
                escape_html(&n.folder)
            ));
        }
        body.push_str("</ul>\n");
        self.page(&name, &body)
    }

    /// Breadcrumbs (site title first) and the note's name, like Obsidian's
    /// inline title.
    fn header(&self, name: &str, crumbs: &[(String, String)], with_root: bool) -> String {
        let mut out = String::from("<header class=\"note-header\">\n");
        if with_root {
            out.push_str("<nav class=\"breadcrumbs\"><ul>");
            out.push_str(&format!(
                "<li><a href=\"/\">{}</a></li>",
                escape_html(self.title)
            ));
            for (label, url) in crumbs {
                out.push_str(&format!(
                    "<li><a href=\"{}\">{}</a></li>",
                    escape_html(url),
                    escape_html(label)
                ));
            }
            out.push_str(&format!("<li>{}</li></ul></nav>\n", escape_html(name)));
        }
        out.push_str(&format!(
            "<div class=\"inline-title\">{}</div>\n</header>\n\n",
            escape_html(name)
        ));
        out
    }

    /// Step 7: front matter (title only, never `tags`) and the body, with
    /// step 6 applied.
    fn page(&self, title: &str, body: &str) -> String {
        self.page_with(title, body, false)
    }

    /// The page at `/`: `home: 'true'` lets the template show its title
    /// alone in the browser tab.
    fn home_page(&self, title: &str, body: &str) -> String {
        self.page_with(title, body, true)
    }

    fn page_with(&self, title: &str, body: &str, home: bool) -> String {
        format!(
            "---\ntitle: {}\nsite_title: {}\n{}---\n\n{}",
            yaml_string(&escape_html(title)),
            yaml_string(&escape_html(self.title)),
            if home { "home: 'true'\n" } else { "" },
            escape_directives(body.trim_end()) + "\n"
        )
    }

    /// The sidebar: the folder tree, a folder linking to its folder note.
    fn nav(&self) -> String {
        let mut out = String::from("<ul class=\"menu nav-tree\">\n");
        self.nav_children("", &mut out);
        out.push_str("<li class=\"nav-tags\"><a href=\"/tags/\">#&nbsp;Tags</a></li>\n");
        out.push_str("</ul>\n");
        out
    }

    /// Like the folder-notes plugin: a folder with a folder note is a link
    /// to it (and the note isn't listed inside), a folder without one is
    /// just a name; only the arrow collapses a folder.
    fn nav_children(&self, path: &str, out: &mut String) {
        let folder = &self.vault.folders[path];
        for sub in &folder.subfolders {
            let child = &self.vault.folders[sub];
            let name = escape_html(&child.name);
            let has_children = !(child.subfolders.is_empty() && child.notes.is_empty());
            let (class, label) = match child.note {
                Some(_) => (
                    "nav-folder has-note",
                    format!(
                        "<a class=\"nav-folder-name\" href=\"{}\">{name}</a>",
                        escape_html(&self.vault.folder_link(sub))
                    ),
                ),
                None => (
                    "nav-folder",
                    format!("<span class=\"nav-folder-name\">{name}</span>"),
                ),
            };
            let toggle = if has_children {
                format!(
                    "<button type=\"button\" class=\"nav-toggle\" aria-expanded=\"false\" aria-label=\"Toggle {name}\"></button>"
                )
            } else {
                "<span class=\"nav-toggle\"></span>".to_owned()
            };
            out.push_str(&format!(
                "<li class=\"{class}\"><div class=\"nav-row\">{toggle}{label}</div>"
            ));
            if has_children {
                out.push_str("\n<ul class=\"nav-children\">\n");
                self.nav_children(sub, out);
                out.push_str("</ul>\n");
            }
            out.push_str("</li>\n");
        }
        for id in &folder.notes {
            if self.vault.home == Some(*id) {
                continue;
            }
            let n = &self.vault.notes[*id];
            out.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>\n",
                escape_html(&n.route),
                escape_html(&n.name)
            ));
        }
    }
}

/// Step 6: render_md treats `{{.` as a directive anywhere, code included;
/// `\{{.` makes it literal text.
pub fn escape_directives(text: &str) -> String {
    text.replace("{{.", "\\{{.")
}

/// A single-quoted YAML scalar.
fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// `src/<route>/index.md` for a route like `/a/b/`.
pub fn page_path(src: &Path, route: &str) -> PathBuf {
    let mut path = src.to_owned();
    for segment in route.split('/').filter(|s| !s.is_empty()) {
        path.push(segment);
    }
    path.join("index.md")
}

pub(crate) fn write(path: &Path, content: &str) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::write(e, parent))?;
    }
    fs::write(path, content).map_err(|e| Error::write(e, path))
}
