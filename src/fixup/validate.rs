//! Buck target validation. Cheap pre-flight to catch typos in fixup
//! `extra_deps` / `replace_deps` before they hit Buck's parser.

/// Valid Buck target: optional `//path` followed by `:name`.
/// Examples:
///   //third-party/c:libjpeg
///   //a/b/c:my_target
///   :local
///   //foo/...        (NOT supported — wildcards aren't fixup-substitutable)
pub fn is_valid_buck_target(s: &str) -> bool {
    // Empty or no colon → invalid.
    let Some(colon_pos) = s.rfind(':') else {
        return false;
    };
    let (pkg, name) = s.split_at(colon_pos);
    let name = &name[1..]; // strip the colon

    // Name must be non-empty and consist of [A-Za-z0-9_./\-].
    if name.is_empty() || !name.bytes().all(is_target_char) {
        return false;
    }

    // Package portion: either empty (`:local` form) or //...
    if pkg.is_empty() {
        return true;
    }
    if !pkg.starts_with("//") {
        return false;
    }
    // Allow [A-Za-z0-9_./\-] in path portion. Reject `..` segments.
    let path = &pkg[2..];
    if path.is_empty() {
        return false;
    }
    for seg in path.split('/') {
        if seg.is_empty() || seg == ".." || seg == "." || seg == "..." {
            return false;
        }
        if !seg.bytes().all(is_target_char) {
            return false;
        }
    }
    true
}

fn is_target_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_forms() {
        assert!(is_valid_buck_target("//third-party/c:libjpeg"));
        assert!(is_valid_buck_target(":local"));
        assert!(is_valid_buck_target("//a/b/c:target"));
        assert!(is_valid_buck_target("//x:y-z_w.0"));
    }

    #[test]
    fn rejects_no_colon() {
        assert!(!is_valid_buck_target("//third-party/c/libjpeg"));
        assert!(!is_valid_buck_target("libjpeg"));
    }

    #[test]
    fn rejects_empty_name() {
        assert!(!is_valid_buck_target("//foo:"));
        assert!(!is_valid_buck_target(":"));
    }

    #[test]
    fn rejects_wildcards() {
        assert!(!is_valid_buck_target("//foo/...:bar"));
        assert!(!is_valid_buck_target("//foo:bar*"));
    }

    #[test]
    fn rejects_traversal() {
        assert!(!is_valid_buck_target("//../foo:bar"));
        assert!(!is_valid_buck_target("//foo/..:bar"));
    }

    #[test]
    fn rejects_missing_leading_slashes() {
        assert!(!is_valid_buck_target("foo:bar"));
        assert!(!is_valid_buck_target("/foo:bar"));
    }
}
