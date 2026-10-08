//! Link resolution with Obsidian's semantics: vault paths, bare names,
//! path suffixes, aliases, heading and block fragments.

use crate::slug;
use crate::vault::{FileId, NoteId, Target, Vault, parent};

#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    Note {
        id: NoteId,
        /// The resolved anchor (`heading-id` or `^block`), if the link has
        /// a fragment that exists.
        anchor: Option<String>,
        /// The fragment as written, if it didn't match anything.
        missing_fragment: Option<String>,
        /// The link was ambiguous; this is the other candidates' paths.
        ambiguous: Vec<String>,
    },
    File(FileId),
    /// The target is an excluded file (e.g. a `.blueprint`): shown as text.
    Excluded,
    Unresolved,
}

impl Vault {
    /// Resolves the target of `[[target]]` written in note `from`.
    pub fn resolve(&self, from: NoteId, target: &str) -> Resolution {
        let (path, fragment) = match target.find('#') {
            Some(i) => (target[..i].trim(), Some(target[i + 1..].trim())),
            None => (target.trim(), None),
        };

        let (found, ambiguous) = if path.is_empty() {
            (Some(Target::Note(from)), Vec::new())
        } else {
            match self.find_target(from, path) {
                Some(found) => found,
                None if self.is_excluded(path) => return Resolution::Excluded,
                None => return Resolution::Unresolved,
            }
        };

        match found {
            Some(Target::File(id)) => Resolution::File(id),
            Some(Target::Note(id)) => {
                let (anchor, missing_fragment) = match fragment.filter(|f| !f.is_empty()) {
                    None => (None, None),
                    Some(fragment) => match self.resolve_fragment(id, fragment) {
                        Some(anchor) => (Some(anchor), None),
                        None => (None, Some(fragment.to_owned())),
                    },
                };
                Resolution::Note {
                    id,
                    anchor,
                    missing_fragment,
                    ambiguous,
                }
            }
            None => Resolution::Unresolved,
        }
    }

    /// Finds the file a link path points at. Returns the target and, if
    /// several candidates matched, the paths of the ones not chosen.
    fn find_target(&self, from: NoteId, path: &str) -> Option<(Option<Target>, Vec<String>)> {
        let from_folder = &self.notes[from].folder;
        let path = path.trim_start_matches('/');
        let lower = path.to_lowercase();

        let candidates: Vec<Target> = if path.starts_with("./") || path.starts_with("../") {
            let joined = normalize(&format!("{from_folder}/{path}")).to_lowercase();
            self.by_path.get(&joined).cloned().unwrap_or_default()
        } else if path.contains('/') {
            match self.by_path.get(&lower) {
                Some(exact) => exact.clone(),
                None => self.by_path_suffix(&lower),
            }
        } else {
            let bare = path.strip_suffix(".md").unwrap_or(path);
            self.by_name
                .get(bare)
                .or_else(|| self.by_name.get(path))
                .or_else(|| self.by_name_lower.get(&bare.to_lowercase()))
                .or_else(|| self.by_name_lower.get(&lower))
                .cloned()
                .unwrap_or_default()
        };
        if candidates.is_empty() {
            return None;
        }
        Some(self.pick(from_folder, candidates))
    }

    /// `[[servers/web]]` matches `lab/servers/web.md`.
    fn by_path_suffix(&self, lower: &str) -> Vec<Target> {
        let suffix = format!("/{lower}");
        let mut found: Vec<Target> = self
            .by_path
            .iter()
            .filter(|(path, _)| path.ends_with(&suffix))
            .flat_map(|(_, targets)| targets.iter().copied())
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// Prefers notes over other files (`smb.conf` is `smb.conf.md` before a
    /// file called `smb.conf`), then the source note's folder, then the
    /// shortest path.
    fn pick(
        &self,
        from_folder: &str,
        mut candidates: Vec<Target>,
    ) -> (Option<Target>, Vec<String>) {
        if candidates.iter().any(|t| matches!(t, Target::Note(_))) {
            candidates.retain(|t| matches!(t, Target::Note(_)));
        }
        let key = |t: &Target| {
            let path = self.target_path(*t);
            let same_folder = parent(path) == from_folder;
            (
                !same_folder,
                path.matches('/').count(),
                path.len(),
                path.to_owned(),
            )
        };
        candidates.sort_by_key(key);
        let chosen = candidates[0];
        let ambiguous = if candidates.len() > 1 {
            let (first, second) = (key(&candidates[0]), key(&candidates[1]));
            if first.0 == second.0 && first.1 == second.1 {
                candidates[1..]
                    .iter()
                    .map(|t| self.target_path(*t).to_owned())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        (Some(chosen), ambiguous)
    }

    fn target_path(&self, target: Target) -> &str {
        match target {
            Target::Note(id) => &self.notes[id].path,
            Target::File(id) => &self.files[id].path,
        }
    }

    fn is_excluded(&self, path: &str) -> bool {
        let lower = path.trim_start_matches('/').to_lowercase();
        self.excluded.contains(&lower) || self.excluded.contains(&format!("{lower}.md"))
    }

    /// Resolves `Heading`, `Heading#Sub` or `^block` against a note.
    pub fn resolve_fragment(&self, id: NoteId, fragment: &str) -> Option<String> {
        let note = &self.notes[id];
        if let Some(block) = fragment.strip_prefix('^') {
            return note
                .block_ids
                .iter()
                .any(|b| b == block)
                .then(|| format!("^{block}"));
        }
        // `[[note#A#B]]` is heading B below A; matching the last part is
        // enough to find it.
        let wanted = fragment.rsplit('#').next().unwrap_or(fragment).trim();
        let wanted_lower = wanted.to_lowercase();
        let wanted_slug = slug::heading_id(wanted);
        note.headings
            .iter()
            .find(|h| h.text == wanted)
            .or_else(|| {
                note.headings
                    .iter()
                    .find(|h| h.text.to_lowercase() == wanted_lower)
            })
            .or_else(|| note.headings.iter().find(|h| h.slug == wanted_slug))
            .map(|h| h.id.clone())
    }

    /// URL of a resolved note link.
    pub fn note_url(&self, from: NoteId, id: NoteId, anchor: Option<&str>) -> String {
        match anchor {
            Some(anchor) if id == from => format!("#{anchor}"),
            Some(anchor) => format!("{}#{anchor}", self.notes[id].route),
            None => self.notes[id].route.clone(),
        }
    }
}

/// Resolves `.` and `..` segments of a `/`-separated path.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(segment),
        }
    }
    out.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostics;
    use crate::vault::ScanOptions;
    use std::fs;

    fn vault(files: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        let vault = Vault::scan(
            dir.path(),
            &ScanOptions {
                excludes: &[],
                home: None,
                title: "Vault",
                skip_dirs: &[],
            },
            &mut Diagnostics::default(),
        )
        .unwrap();
        (dir, vault)
    }

    fn note_id(vault: &Vault, path: &str) -> NoteId {
        vault.find_note_by_path(path).unwrap()
    }

    fn resolved_path(vault: &Vault, from: &str, target: &str) -> Option<String> {
        match vault.resolve(note_id(vault, from), target) {
            Resolution::Note { id, .. } => Some(vault.notes[id].path.clone()),
            Resolution::File(id) => Some(vault.files[id].path.clone()),
            _ => None,
        }
    }

    #[test]
    fn test_bare_names_paths_and_suffixes() {
        let (_dir, v) = vault(&[
            ("a/x.md", ""),
            ("a/b/y.md", ""),
            ("c/Project/Project.md", ""),
            ("d/Project/Project.md", ""),
            ("a/smb.conf.md", ""),
            ("a/data.base", ""),
        ]);
        assert_eq!(
            resolved_path(&v, "a/x.md", "y").as_deref(),
            Some("a/b/y.md")
        );
        assert_eq!(
            resolved_path(&v, "a/x.md", "Y").as_deref(),
            Some("a/b/y.md")
        );
        assert_eq!(
            resolved_path(&v, "a/x.md", "a/b/y.md").as_deref(),
            Some("a/b/y.md")
        );
        assert_eq!(
            resolved_path(&v, "a/x.md", "b/y").as_deref(),
            Some("a/b/y.md")
        );
        assert_eq!(
            resolved_path(&v, "a/x.md", "./b/y").as_deref(),
            Some("a/b/y.md")
        );
        assert_eq!(
            resolved_path(&v, "a/x.md", "d/Project/Project").as_deref(),
            Some("d/Project/Project.md")
        );
        assert_eq!(
            resolved_path(&v, "a/x.md", "smb.conf").as_deref(),
            Some("a/smb.conf.md")
        );
        assert_eq!(
            resolved_path(&v, "a/x.md", "data.base").as_deref(),
            Some("a/data.base")
        );
        assert_eq!(resolved_path(&v, "a/x.md", "nope"), None);
    }

    #[test]
    fn test_ambiguous_names_prefer_the_source_folder() {
        let (_dir, v) = vault(&[
            ("c/Project/Project.md", ""),
            ("c/Project/Board.md", ""),
            ("d/Project/Project.md", ""),
            ("d/Project/Board.md", ""),
        ]);
        assert_eq!(
            resolved_path(&v, "d/Project/Project.md", "Board").as_deref(),
            Some("d/Project/Board.md")
        );
        let Resolution::Note { ambiguous, .. } =
            v.resolve(note_id(&v, "c/Project/Board.md"), "Project")
        else {
            panic!()
        };
        assert!(ambiguous.is_empty());
    }

    #[test]
    fn test_fragments() {
        let (_dir, v) = vault(&[
            (
                "n.md",
                "# Login Requests (sso / proxy)\n\ntext ^blk\n\n## Notes\n## Notes\n",
            ),
            ("m.md", ""),
        ]);
        let from = note_id(&v, "m.md");
        let anchor = |target: &str| match v.resolve(from, target) {
            Resolution::Note { anchor, .. } => anchor,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            anchor("n#Login Requests (sso / proxy)").as_deref(),
            Some("login-requests-sso--proxy")
        );
        assert_eq!(
            anchor("n#login requests (SSO / proxy)").as_deref(),
            Some("login-requests-sso--proxy")
        );
        assert_eq!(anchor("n#^blk").as_deref(), Some("^blk"));
        assert_eq!(anchor("n#Notes").as_deref(), Some("notes"));
        assert_eq!(anchor("n#^missing"), None);
        let Resolution::Note {
            missing_fragment, ..
        } = v.resolve(from, "n#Nope")
        else {
            panic!()
        };
        assert_eq!(missing_fragment.as_deref(), Some("Nope"));
    }

    #[test]
    fn test_excluded_targets() {
        let (_dir, v) = vault(&[("a/n.md", ""), ("a/lxc.blueprint", "")]);
        assert_eq!(
            v.resolve(note_id(&v, "a/n.md"), "lxc.blueprint"),
            Resolution::Excluded
        );
        assert_eq!(
            v.resolve(note_id(&v, "a/n.md"), "other"),
            Resolution::Unresolved
        );
    }
}
