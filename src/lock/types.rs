//! Lockfile domain types.
//!
//! Mirrors the shape of `uv.lock` but typed and normalized. The parser
//! (`src/lock/parser.rs`) converts the raw TOML into these.

use pep440_rs::Version;
use pep508_rs::{MarkerTree, PackageName};
use url::Url;

// ---- Types ----

#[derive(Debug, Clone)]
pub struct Package {
    pub name: PackageName,
    pub version: Version,
    pub source: Source,
    pub dependencies: Vec<DepEdge>,
    pub sdist: Option<Sdist>,
    pub wheels: Vec<Wheel>,
    pub metadata: Option<Metadata>,
}

#[derive(Debug, Clone)]
pub enum Source {
    Registry {
        url: Url,
    },
    Git {
        url: Url,
        rev: String,
        subdirectory: Option<String>,
    },
    FirstParty {
        kind: FirstPartyKind,
        path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstPartyKind {
    Virtual,
    Editable,
    Directory,
    Path,
}

#[derive(Debug, Clone)]
pub struct DepEdge {
    pub name: PackageName,
    pub extra: Vec<String>,
    pub marker: Option<MarkerTree>,
}

#[derive(Debug, Clone)]
pub struct Sdist {
    pub url: Url,
    pub hash: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Wheel {
    pub url: Url,
    pub hash: String,
    pub size: Option<u64>,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct Metadata {
    pub requires_dist: Vec<DepEdge>,
    pub provides_extras: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Lockfile {
    pub version: u32,
    pub revision: u32,
    pub requires_python: String,
    pub packages: Vec<Package>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn first_party_kind_round_trips() {
        let k = FirstPartyKind::Editable;
        assert_eq!(k, FirstPartyKind::Editable);
        assert_ne!(k, FirstPartyKind::Virtual);
    }

    #[test]
    fn package_constructs() {
        let pkg = Package {
            name: PackageName::from_str("numpy").unwrap(),
            version: Version::from_str("2.4.6").unwrap(),
            source: Source::Registry {
                url: Url::parse("https://pypi.org/simple").unwrap(),
            },
            dependencies: vec![],
            sdist: None,
            wheels: vec![],
            metadata: None,
        };
        assert_eq!(pkg.name.as_ref(), "numpy");
        assert_eq!(pkg.version.to_string(), "2.4.6");
    }
}
