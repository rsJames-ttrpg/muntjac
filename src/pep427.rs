//! PEP 427 wheel filename naming utilities.
//!
//! See <https://peps.python.org/pep-0427/#file-name-convention>.

/// PEP 427 escape rule for the distribution name component of a wheel
/// filename: lowercase, and collapse runs of non-alphanumeric characters
/// to a single underscore.
pub fn escape_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_sep = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    out
}

/// PEP 427 wheel filename for a pure-python distribution:
/// `{escape_name(name)}-{version}-py3-none-any.whl`.
///
/// Caller must ensure `version` is already PEP 440 canonical; epochs and
/// local segments are not normalized here.
pub fn pure_python_filename(name: &str, version: &str) -> String {
    format!("{}-{}-py3-none-any.whl", escape_name(name), version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_dashes_dots_lowercases() {
        assert_eq!(escape_name("Flit-Core"), "flit_core");
        assert_eq!(escape_name("foo.bar"), "foo_bar");
        assert_eq!(escape_name("foo---bar"), "foo_bar");
        assert_eq!(escape_name("urllib3"), "urllib3");
    }

    #[test]
    fn pure_python_filename_format() {
        assert_eq!(
            pure_python_filename("Flit-Core", "3.9.0"),
            "flit_core-3.9.0-py3-none-any.whl"
        );
    }
}
