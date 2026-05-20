//! PEP 425/600/656 wheel tag types and filename parser.

/// A single fully-expanded PEP 425 tag triple.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    pub python: PythonTag,
    pub abi: AbiTag,
    pub plat: PlatformTag,
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
    ManyLinux { major: u32, minor: u32, arch: LinuxArch },
    MuslLinux { major: u32, minor: u32, arch: LinuxArch },
    MacOs    { major: u32, minor: u32, arch: MacArch },
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LinuxArch { X86_64, Aarch64 }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MacArch { X86_64, Arm64, Universal2 }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_equality_and_hash() {
        let a = Tag {
            python: PythonTag::CPython(3, 12),
            abi:    AbiTag::CPython(3, 12),
            plat:   PlatformTag::ManyLinux { major: 2, minor: 17, arch: LinuxArch::X86_64 },
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
}
