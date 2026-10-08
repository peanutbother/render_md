//! Minimal glob matching for `--exclude`, gitignore-flavoured: a pattern
//! without `/` matches any file or folder name (`*.blueprint`, `drafts`), a
//! pattern with `/` matches vault-relative paths (`lab/old/**`). A
//! match on a folder excludes everything below it.

/// A compiled `--exclude` pattern.
#[derive(Debug, Clone)]
pub struct Glob {
    pattern: Vec<char>,
    /// Whether the pattern contains a `/` (after trimming a trailing one),
    /// i.e. is matched against whole paths rather than single names.
    anchored: bool,
}

impl Glob {
    pub fn new(pattern: &str) -> Self {
        let trimmed = pattern
            .trim()
            .trim_start_matches("./")
            .trim_start_matches('/')
            .trim_end_matches('/');
        Self {
            pattern: trimmed.chars().collect(),
            anchored: trimmed.contains('/'),
        }
    }

    /// Whether `path` (vault-relative, `/`-separated) or one of the
    /// folders it lives in is matched.
    pub fn matches(&self, path: &str) -> bool {
        let segments: Vec<&str> = path.split('/').collect();
        (1..=segments.len()).any(|n| {
            if self.anchored {
                self.matches_str(&segments[..n].join("/"))
            } else {
                self.matches_str(segments[n - 1])
            }
        })
    }

    fn matches_str(&self, candidate: &str) -> bool {
        let candidate: Vec<char> = candidate.chars().collect();
        wildmatch(&self.pattern, &candidate)
    }
}

/// `*` matches within one segment, `**` across segments (`a/**/b` also
/// matches `a/b`), `?` matches one non-`/` character.
fn wildmatch(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) if rest.first() == Some(&'*') => {
            let rest = &rest[1..];
            // `**/` may also match zero folders.
            if rest.first() == Some(&'/') && wildmatch(&rest[1..], text) {
                return true;
            }
            (0..=text.len()).any(|i| wildmatch(rest, &text[i..]))
        }
        Some(('*', rest)) => {
            for i in 0..=text.len() {
                if wildmatch(rest, &text[i..]) {
                    return true;
                }
                if text.get(i) == Some(&'/') {
                    break;
                }
            }
            false
        }
        Some(('?', rest)) => match text.split_first() {
            Some((c, text_rest)) if *c != '/' => wildmatch(rest, text_rest),
            _ => false,
        },
        Some((p, rest)) => match text.split_first() {
            Some((c, text_rest)) if c == p => wildmatch(rest, text_rest),
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unanchored_pattern_matches_any_name() {
        let glob = Glob::new("*.blueprint");
        assert!(glob.matches("lab/lxc.blueprint"));
        assert!(glob.matches("vm.blueprint"));
        assert!(!glob.matches("lab/lxc.md"));
    }

    #[test]
    fn test_folder_name_excludes_contents() {
        let glob = Glob::new("drafts/");
        assert!(glob.matches("drafts/a.md"));
        assert!(glob.matches("notes/drafts/b.md"));
        assert!(!glob.matches("notes/drafts.md"));
    }

    #[test]
    fn test_anchored_pattern_matches_paths() {
        let glob = Glob::new("lab/old/**");
        assert!(glob.matches("lab/old/a.md"));
        assert!(glob.matches("lab/old/x/y.md"));
        assert!(!glob.matches("old/a.md"));

        let glob = Glob::new("a/*/c.md");
        assert!(glob.matches("a/b/c.md"));
        assert!(!glob.matches("a/b/x/c.md"));
    }

    #[test]
    fn test_double_star_matches_zero_folders() {
        let glob = Glob::new("a/**/b.md");
        assert!(glob.matches("a/b.md"));
        assert!(glob.matches("a/x/y/b.md"));
    }

    #[test]
    fn test_question_mark_matches_one_char() {
        let glob = Glob::new("note?.md");
        assert!(glob.matches("dir/note1.md"));
        assert!(!glob.matches("dir/note12.md"));
    }
}
