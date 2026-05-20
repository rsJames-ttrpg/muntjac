//! Compatible-tag list construction and ranking.

use super::tag::Tag;
use std::collections::HashMap;

pub struct CompatibleTags {
    ordered: Vec<Tag>,
    by_tag: HashMap<Tag, usize>,
}

impl CompatibleTags {
    pub(crate) fn from_ordered(ordered: Vec<Tag>) -> Self {
        let by_tag = ordered.iter().enumerate().map(|(i, t)| (t.clone(), i)).collect();
        Self { ordered, by_tag }
    }

    pub fn rank_of(&self, tag: &Tag) -> Option<usize> {
        self.by_tag.get(tag).copied()
    }

    pub fn ordered(&self) -> &[Tag] {
        &self.ordered
    }

    pub fn len(&self) -> usize {
        self.ordered.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tag::{PythonTag, AbiTag, PlatformTag, LinuxArch};

    #[test]
    fn rank_of_returns_index() {
        let tag_a = Tag {
            python: PythonTag::CPython(3, 12),
            abi:    AbiTag::CPython(3, 12),
            plat:   PlatformTag::Any,
        };
        let tag_b = Tag {
            python: PythonTag::Py(3, None),
            abi:    AbiTag::None,
            plat:   PlatformTag::Any,
        };
        let compat = CompatibleTags::from_ordered(vec![tag_a.clone(), tag_b.clone()]);
        assert_eq!(compat.rank_of(&tag_a), Some(0));
        assert_eq!(compat.rank_of(&tag_b), Some(1));

        let unknown = Tag {
            python: PythonTag::CPython(3, 10),
            abi:    AbiTag::None,
            plat:   PlatformTag::ManyLinux { major: 2, minor: 17, arch: LinuxArch::X86_64 },
        };
        assert_eq!(compat.rank_of(&unknown), None);
    }
}
