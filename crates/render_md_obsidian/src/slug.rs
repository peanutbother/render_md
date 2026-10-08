//! The one slug function used for URL path segments, heading ids and tag
//! pages, so a link and the anchor or page it points at always agree.

/// Slug for a URL path segment (a folder or note name). Keeps `.`, because
/// note names like `smb.conf` or `python 3.14` contain meaningful dots.
///
/// Never returns an empty string or a dot-only segment (`.`, `..`), which
/// would escape the output directory: such names fall back to a stable hash.
pub fn path_segment(name: &str) -> String {
    let slug = slugify(name, true);
    if slug.is_empty() || slug.chars().all(|c| c == '.') {
        format!("_{:08x}", fnv1a(name))
    } else {
        slug
    }
}

/// Slug for a heading id. `Login Requests (sso / proxy)` becomes
/// `login-requests-sso--proxy`, like GitHub's heading anchors. A heading
/// without any usable character becomes `section`.
pub fn heading_id(text: &str) -> String {
    let slug = slugify(text, false);
    if slug.is_empty() {
        "section".to_owned()
    } else {
        slug
    }
}

/// Lowercases, keeps alphanumerics, `-` and `_` (and `.` if `keep_dots`),
/// turns whitespace into `-` and drops everything else.
fn slugify(text: &str, keep_dots: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c.is_whitespace() {
            out.push('-');
        } else if c == '-' || c == '_' || (keep_dots && c == '.') {
            out.push(c);
        }
    }
    out
}

fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// Assigns unique heading ids within one page: the first `foo` stays `foo`,
/// later ones become `foo-1`, `foo-2`, ...
#[derive(Debug, Default, Clone)]
pub struct IdRegistry {
    used: std::collections::HashSet<String>,
}

impl IdRegistry {
    /// Returns `base`, or `base-N` for the smallest `N` not used yet, and
    /// marks the result as used.
    pub fn assign(&mut self, base: &str) -> String {
        if self.used.insert(base.to_owned()) {
            return base.to_owned();
        }
        let mut n = 1;
        loop {
            let candidate = format!("{base}-{n}");
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }

    /// Marks an explicitly written id (`# Title {#custom}`) as used.
    pub fn reserve(&mut self, id: &str) {
        self.used.insert(id.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading_id_drops_punctuation_and_keeps_double_dash() {
        assert_eq!(
            heading_id("Login Requests (sso / proxy)"),
            "login-requests-sso--proxy"
        );
    }

    #[test]
    fn test_heading_id_drops_dots() {
        assert_eq!(heading_id("Python 3.14"), "python-314");
    }

    #[test]
    fn test_path_segment_keeps_dots_underscores_and_unicode() {
        assert_eq!(path_segment("smb.conf"), "smb.conf");
        assert_eq!(
            path_segment("fix mk_docker python 3.14"),
            "fix-mk_docker-python-3.14"
        );
        assert_eq!(path_segment("Größe"), "größe");
    }

    #[test]
    fn test_path_segment_never_returns_dot_or_empty_segments() {
        for name in ["..", ".", "???", ""] {
            let slug = path_segment(name);
            assert!(slug.starts_with('_'), "{name:?} -> {slug:?}");
            assert!(!slug.chars().all(|c| c == '.'));
        }
        assert_ne!(path_segment(".."), path_segment("???"));
    }

    #[test]
    fn test_heading_id_falls_back_to_section() {
        assert_eq!(heading_id("???"), "section");
    }

    #[test]
    fn test_id_registry_suffixes_duplicates() {
        let mut ids = IdRegistry::default();
        assert_eq!(ids.assign("notes"), "notes");
        assert_eq!(ids.assign("notes"), "notes-1");
        assert_eq!(ids.assign("notes"), "notes-2");
        ids.reserve("other-1");
        assert_eq!(ids.assign("other"), "other");
        assert_eq!(ids.assign("other"), "other-2");
    }
}
