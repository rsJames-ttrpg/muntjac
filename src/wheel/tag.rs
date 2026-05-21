//! PEP 425/600/656 wheel tag types and filename parser.

use std::fmt::{self, Display, Formatter};

/// A single fully-expanded PEP 425 tag triple.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    pub python: PythonTag,
    pub abi: AbiTag,
    pub plat: PlatformTag,
}

impl Display for Tag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.python, self.abi, self.plat)
    }
}

impl Display for PythonTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            PythonTag::CPython(maj, min) => write!(f, "cp{maj}{min}"),
            PythonTag::Py(maj, Some(min)) => write!(f, "py{maj}{min}"),
            PythonTag::Py(maj, None) => write!(f, "py{maj}"),
            PythonTag::Other(s) => write!(f, "{s}"),
        }
    }
}

impl Display for AbiTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            AbiTag::CPython(maj, min) => write!(f, "cp{maj}{min}"),
            AbiTag::Abi3 => write!(f, "abi3"),
            AbiTag::None => write!(f, "none"),
            AbiTag::Other(s) => write!(f, "{s}"),
        }
    }
}

impl Display for PlatformTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            PlatformTag::Any => write!(f, "any"),
            PlatformTag::ManyLinux { major, minor, arch } => {
                let a = match arch {
                    LinuxArch::X86_64 => "x86_64",
                    LinuxArch::Aarch64 => "aarch64",
                };
                write!(f, "manylinux_{major}_{minor}_{a}")
            }
            PlatformTag::MuslLinux { major, minor, arch } => {
                let a = match arch {
                    LinuxArch::X86_64 => "x86_64",
                    LinuxArch::Aarch64 => "aarch64",
                };
                write!(f, "musllinux_{major}_{minor}_{a}")
            }
            PlatformTag::MacOs { major, minor, arch } => {
                let a = match arch {
                    MacArch::X86_64 => "x86_64",
                    MacArch::Arm64 => "arm64",
                    MacArch::Universal2 => "universal2",
                };
                write!(f, "macosx_{major}_{minor}_{a}")
            }
            PlatformTag::Other(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PythonTag {
    /// e.g. cp312 → CPython(3, 12)
    CPython(u8, u8),
    /// `py3` → Py(3, None); `py37` → Py(3, Some(7)); `py2` → Py(2, None).
    Py(u8, Option<u8>),
    /// pp310, jy27, ip3 — unsupported, kept opaque.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AbiTag {
    CPython(u8, u8),
    Abi3,
    None,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlatformTag {
    Any,
    ManyLinux {
        major: u32,
        minor: u32,
        arch: LinuxArch,
    },
    MuslLinux {
        major: u32,
        minor: u32,
        arch: LinuxArch,
    },
    MacOs {
        major: u32,
        minor: u32,
        arch: MacArch,
    },
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LinuxArch {
    X86_64,
    Aarch64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MacArch {
    X86_64,
    Arm64,
    Universal2,
}

/// All tags expanded from a wheel filename.
#[derive(Debug, Clone)]
pub struct WheelTag {
    pub tags: Vec<Tag>,
    pub raw_filename: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TagParseError {
    #[error("wheel filename `{0}` does not match PEP 427 structure")]
    MalformedFilename(String),
    #[error("unknown structure in tag segment of `{0}`")]
    UnknownTagShape(String),
}

/// Parse a PEP 427 wheel filename into all its compatible Tag triples.
pub fn parse_filename(filename: &str) -> Result<WheelTag, TagParseError> {
    let stem = filename
        .strip_suffix(".whl")
        .ok_or_else(|| TagParseError::MalformedFilename(filename.to_string()))?;

    let parts: Vec<&str> = stem.split('-').collect();
    let (py_seg, abi_seg, plat_seg) = match parts.len() {
        n if n < 5 => return Err(TagParseError::MalformedFilename(filename.to_string())),
        5 => (parts[2], parts[3], parts[4]),
        _ => {
            let n = parts.len();
            (parts[n - 3], parts[n - 2], parts[n - 1])
        }
    };

    let pythons = py_seg
        .split('.')
        .map(parse_python_tag)
        .collect::<Result<Vec<_>, _>>()?;
    let abis = abi_seg
        .split('.')
        .map(parse_abi_tag)
        .collect::<Result<Vec<_>, _>>()?;
    let plats = plat_seg
        .split('.')
        .map(parse_platform_tag)
        .collect::<Result<Vec<_>, _>>()?;

    let mut tags = Vec::new();
    for py in &pythons {
        for abi in &abis {
            for plat in &plats {
                let t = Tag {
                    python: py.clone(),
                    abi: abi.clone(),
                    plat: plat.clone(),
                };
                if !tags.contains(&t) {
                    tags.push(t);
                }
            }
        }
    }

    Ok(WheelTag {
        tags,
        raw_filename: filename.to_string(),
    })
}

fn parse_python_tag(s: &str) -> Result<PythonTag, TagParseError> {
    if let Some(rest) = s.strip_prefix("cp") {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let trailing = &rest[digits.len()..];
        if trailing.is_empty() {
            if let Some((maj, min)) = parse_version_digits(&digits) {
                return Ok(PythonTag::CPython(maj, min.unwrap_or(0)));
            }
        }
        return Ok(PythonTag::Other(s.to_string()));
    }
    if let Some(rest) = s.strip_prefix("py") {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let trailing = &rest[digits.len()..];
        if trailing.is_empty() {
            if let Some((maj, min)) = parse_version_digits(&digits) {
                return Ok(PythonTag::Py(maj, min));
            }
        }
        return Ok(PythonTag::Other(s.to_string()));
    }
    Ok(PythonTag::Other(s.to_string()))
}

fn parse_abi_tag(s: &str) -> Result<AbiTag, TagParseError> {
    match s {
        "abi3" => Ok(AbiTag::Abi3),
        "none" => Ok(AbiTag::None),
        _ => {
            if let Some(rest) = s.strip_prefix("cp") {
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                let trailing = &rest[digits.len()..];
                if trailing.is_empty() {
                    if let Some((maj, min)) = parse_version_digits(&digits) {
                        return Ok(AbiTag::CPython(maj, min.unwrap_or(0)));
                    }
                }
                // Any trailing non-digit suffix (e.g. 'd', 'm', 't') means
                // we don't recognize the exact ABI — keep opaque.
            }
            Ok(AbiTag::Other(s.to_string()))
        }
    }
}

fn parse_platform_tag(s: &str) -> Result<PlatformTag, TagParseError> {
    if s == "any" {
        return Ok(PlatformTag::Any);
    }
    if let Some(arch) = s.strip_prefix("manylinux1_") {
        return Ok(PlatformTag::ManyLinux {
            major: 2,
            minor: 5,
            arch: parse_linux_arch(arch)?,
        });
    }
    if let Some(arch) = s.strip_prefix("manylinux2010_") {
        return Ok(PlatformTag::ManyLinux {
            major: 2,
            minor: 12,
            arch: parse_linux_arch(arch)?,
        });
    }
    if let Some(arch) = s.strip_prefix("manylinux2014_") {
        return Ok(PlatformTag::ManyLinux {
            major: 2,
            minor: 17,
            arch: parse_linux_arch(arch)?,
        });
    }
    if let Some(rest) = s.strip_prefix("manylinux_") {
        if let Some((maj, min, arch)) = split_versioned_linux(rest) {
            return Ok(PlatformTag::ManyLinux {
                major: maj,
                minor: min,
                arch,
            });
        }
        // fall through to Other for unknown arches (s390x, ppc64le, armv7l, etc.)
    }
    if let Some(rest) = s.strip_prefix("musllinux_") {
        if let Some((maj, min, arch)) = split_versioned_linux(rest) {
            return Ok(PlatformTag::MuslLinux {
                major: maj,
                minor: min,
                arch,
            });
        }
    }
    if let Some(rest) = s.strip_prefix("macosx_") {
        if let Some((maj, min, arch)) = split_versioned_mac(rest) {
            return Ok(PlatformTag::MacOs {
                major: maj,
                minor: min,
                arch,
            });
        }
    }
    Ok(PlatformTag::Other(s.to_string()))
}

fn parse_linux_arch(s: &str) -> Result<LinuxArch, TagParseError> {
    match s {
        "x86_64" => Ok(LinuxArch::X86_64),
        "aarch64" => Ok(LinuxArch::Aarch64),
        _ => Err(TagParseError::UnknownTagShape(s.into())),
    }
}

/// Parse `<major>_<minor>_<arch>` for manylinux/musllinux. (Note: `x86_64` itself contains
/// an underscore, but `splitn(3, '_')` correctly captures the first two ints + arch tail.)
fn split_versioned_linux(s: &str) -> Option<(u32, u32, LinuxArch)> {
    let mut parts = s.splitn(3, '_');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let arch_str = parts.next()?;
    let arch = match arch_str {
        "x86_64" => LinuxArch::X86_64,
        "aarch64" => LinuxArch::Aarch64,
        _ => return None,
    };
    Some((major, minor, arch))
}

fn split_versioned_mac(s: &str) -> Option<(u32, u32, MacArch)> {
    let mut parts = s.splitn(3, '_');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let arch_str = parts.next()?;
    let arch = match arch_str {
        "x86_64" => MacArch::X86_64,
        "arm64" => MacArch::Arm64,
        "universal2" => MacArch::Universal2,
        _ => return None,
    };
    Some((major, minor, arch))
}

/// Parse the digit string after `cp` or `py`. Returns (major, optional minor).
/// `"3"` → (3, None); `"312"` → (3, Some(12)); `"37"` → (3, Some(7)).
fn parse_version_digits(s: &str) -> Option<(u8, Option<u8>)> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let major: u8 = s[..1].parse().ok()?;
    if s.len() == 1 {
        return Some((major, None));
    }
    let minor: u8 = s[1..].parse().ok()?;
    Some((major, Some(minor)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_equality_and_hash() {
        let a = Tag {
            python: PythonTag::CPython(3, 12),
            abi: AbiTag::CPython(3, 12),
            plat: PlatformTag::ManyLinux {
                major: 2,
                minor: 17,
                arch: LinuxArch::X86_64,
            },
        };
        let b = a.clone();
        assert_eq!(a, b);

        let mut set = std::collections::HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn arch_variants_are_distinct() {
        assert_ne!(LinuxArch::X86_64, LinuxArch::Aarch64);
        assert_ne!(MacArch::X86_64, MacArch::Arm64);
        assert_ne!(MacArch::Arm64, MacArch::Universal2);
    }

    fn parse(s: &str) -> WheelTag {
        parse_filename(s).unwrap_or_else(|e| panic!("parse `{s}`: {e}"))
    }

    #[test]
    fn parses_simple_cp312_manylinux() {
        let w = parse("numpy-2.1.3-cp312-cp312-manylinux_2_17_x86_64.whl");
        assert_eq!(w.tags.len(), 1);
        assert_eq!(
            w.tags[0],
            Tag {
                python: PythonTag::CPython(3, 12),
                abi: AbiTag::CPython(3, 12),
                plat: PlatformTag::ManyLinux {
                    major: 2,
                    minor: 17,
                    arch: LinuxArch::X86_64
                },
            }
        );
    }

    #[test]
    fn parses_pure_python_any() {
        let w = parse("requests-2.32.3-py3-none-any.whl");
        assert_eq!(w.tags.len(), 1);
        assert_eq!(
            w.tags[0],
            Tag {
                python: PythonTag::Py(3, None),
                abi: AbiTag::None,
                plat: PlatformTag::Any,
            }
        );
    }

    #[test]
    fn parses_abi3_wheel() {
        let w = parse("cryptography-42.0.0-cp37-abi3-manylinux_2_28_x86_64.whl");
        assert_eq!(w.tags[0].abi, AbiTag::Abi3);
    }

    #[test]
    fn expands_compressed_python_tag() {
        let w = parse("foo-1.0-py2.py3-none-any.whl");
        assert_eq!(w.tags.len(), 2);
        assert!(w.tags.contains(&Tag {
            python: PythonTag::Py(2, None),
            abi: AbiTag::None,
            plat: PlatformTag::Any
        }));
        assert!(w.tags.contains(&Tag {
            python: PythonTag::Py(3, None),
            abi: AbiTag::None,
            plat: PlatformTag::Any
        }));
    }

    #[test]
    fn canonicalizes_manylinux2014_alias() {
        let w = parse("foo-1.0-cp312-cp312-manylinux2014_x86_64.whl");
        assert_eq!(
            w.tags[0].plat,
            PlatformTag::ManyLinux {
                major: 2,
                minor: 17,
                arch: LinuxArch::X86_64
            }
        );
    }

    #[test]
    fn dedupes_after_alias_canonicalization() {
        let w = parse("foo-1.0-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl");
        assert_eq!(w.tags.len(), 1);
    }

    #[test]
    fn parses_macos_universal2() {
        let w = parse("foo-1.0-cp312-cp312-macosx_11_0_universal2.whl");
        assert_eq!(
            w.tags[0].plat,
            PlatformTag::MacOs {
                major: 11,
                minor: 0,
                arch: MacArch::Universal2
            }
        );
    }

    #[test]
    fn parses_musllinux() {
        let w = parse("foo-1.0-cp312-cp312-musllinux_1_2_x86_64.whl");
        assert_eq!(
            w.tags[0].plat,
            PlatformTag::MuslLinux {
                major: 1,
                minor: 2,
                arch: LinuxArch::X86_64
            }
        );
    }

    #[test]
    fn pypy_tag_classified_as_other() {
        let w = parse("foo-1.0-pp310-pypy310_pp73-manylinux_2_17_x86_64.whl");
        assert!(matches!(w.tags[0].python, PythonTag::Other(ref s) if s == "pp310"));
    }

    #[test]
    fn build_tag_is_ignored() {
        let w = parse("foo-1.0-1-cp312-cp312-manylinux_2_17_x86_64.whl");
        assert_eq!(w.tags[0].python, PythonTag::CPython(3, 12));
    }

    #[test]
    fn rejects_missing_whl_suffix() {
        assert!(matches!(
            parse_filename("foo-1.0-py3-none-any.zip"),
            Err(TagParseError::MalformedFilename(_))
        ));
    }

    #[test]
    fn rejects_too_few_segments() {
        assert!(matches!(
            parse_filename("foo-1.0.whl"),
            Err(TagParseError::MalformedFilename(_))
        ));
    }

    #[test]
    fn s390x_manylinux_falls_to_other() {
        let w = parse("cryptography-43.0.0-cp312-cp312-manylinux_2_28_s390x.whl");
        assert!(matches!(w.tags[0].plat, PlatformTag::Other(ref s)
            if s == "manylinux_2_28_s390x"));
    }

    #[test]
    fn armv7l_manylinux_falls_to_other() {
        let w = parse("foo-1.0-cp312-cp312-manylinux_2_28_armv7l.whl");
        assert!(matches!(w.tags[0].plat, PlatformTag::Other(_)));
    }

    #[test]
    fn cp313t_free_threaded_routes_to_other() {
        let w = parse("foo-1.0-cp313t-cp313t-manylinux_2_28_x86_64.whl");
        assert!(matches!(w.tags[0].python, PythonTag::Other(ref s) if s == "cp313t"));
        assert!(matches!(w.tags[0].abi, AbiTag::Other(ref s) if s == "cp313t"));
    }

    #[test]
    fn tag_display_canonical_cases() {
        let cp312 = Tag {
            python: PythonTag::CPython(3, 12),
            abi: AbiTag::CPython(3, 12),
            plat: PlatformTag::ManyLinux {
                major: 2,
                minor: 17,
                arch: LinuxArch::X86_64,
            },
        };
        assert_eq!(cp312.to_string(), "cp312-cp312-manylinux_2_17_x86_64");

        let abi3 = Tag {
            python: PythonTag::CPython(3, 12),
            abi: AbiTag::Abi3,
            plat: PlatformTag::MacOs {
                major: 11,
                minor: 0,
                arch: MacArch::Universal2,
            },
        };
        assert_eq!(abi3.to_string(), "cp312-abi3-macosx_11_0_universal2");

        let pure = Tag {
            python: PythonTag::Py(3, None),
            abi: AbiTag::None,
            plat: PlatformTag::Any,
        };
        assert_eq!(pure.to_string(), "py3-none-any");

        let py_minor = Tag {
            python: PythonTag::Py(3, Some(7)),
            abi: AbiTag::None,
            plat: PlatformTag::Any,
        };
        assert_eq!(py_minor.to_string(), "py37-none-any");

        let other = Tag {
            python: PythonTag::Other("pp310".into()),
            abi: AbiTag::Other("pypy310_pp73".into()),
            plat: PlatformTag::MuslLinux {
                major: 1,
                minor: 2,
                arch: LinuxArch::Aarch64,
            },
        };
        assert_eq!(
            other.to_string(),
            "pp310-pypy310_pp73-musllinux_1_2_aarch64"
        );
    }
}
