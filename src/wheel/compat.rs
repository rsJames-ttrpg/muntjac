//! Compatible-tag list construction and ranking.

use super::tag::{AbiTag, PythonTag, Tag};
use crate::config::PythonVersion;
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

pub(crate) fn build_python_axis(py: PythonVersion) -> Vec<(PythonTag, AbiTag)> {
    let major = py.0;
    let minor = py.1;
    let mut out = Vec::new();

    // 1-3: Current interpreter, full / stable-ABI / no-ABI.
    out.push((PythonTag::CPython(major, minor), AbiTag::CPython(major, minor)));
    out.push((PythonTag::CPython(major, minor), AbiTag::Abi3));
    out.push((PythonTag::CPython(major, minor), AbiTag::None));

    // 4: Older cp3X-abi3, walking back from (minor-1) down to 2.
    //    PEP 384 abi3 introduced in 3.2.
    if minor >= 3 {
        for older in (2..minor).rev() {
            out.push((PythonTag::CPython(major, older), AbiTag::Abi3));
        }
    }

    // 5: py3X-none from current minor down to 0.
    for older in (0..=minor).rev() {
        out.push((PythonTag::Py(major, Some(older)), AbiTag::None));
    }

    // 6: py3-none (catch-all).
    out.push((PythonTag::Py(major, None), AbiTag::None));

    out
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

    #[test]
    fn python_axis_for_3_12() {
        use crate::config::PythonVersion;
        let axis = build_python_axis(PythonVersion(3, 12));

        // The first three are the current-interpreter triple.
        assert_eq!(axis[0], (PythonTag::CPython(3, 12), AbiTag::CPython(3, 12)));
        assert_eq!(axis[1], (PythonTag::CPython(3, 12), AbiTag::Abi3));
        assert_eq!(axis[2], (PythonTag::CPython(3, 12), AbiTag::None));

        // Older cp_abi3 entries: cp311-abi3 ... cp32-abi3 (10 entries).
        assert_eq!(axis[3], (PythonTag::CPython(3, 11), AbiTag::Abi3));
        assert_eq!(axis[12], (PythonTag::CPython(3, 2), AbiTag::Abi3));

        // Then py3<minor>-none walking down to py30-none.
        assert_eq!(axis[13], (PythonTag::Py(3, Some(12)), AbiTag::None));
        assert_eq!(axis[25], (PythonTag::Py(3, Some(0)), AbiTag::None));

        // Then py3-none.
        assert_eq!(axis[26], (PythonTag::Py(3, None), AbiTag::None));

        // Total length: 3 (current) + 10 (older abi3) + 13 (py3X) + 1 (py3) = 27.
        assert_eq!(axis.len(), 27);
    }

    #[test]
    fn python_axis_for_3_8() {
        use crate::config::PythonVersion;
        let axis = build_python_axis(PythonVersion(3, 8));
        // 3 (current) + 6 (older abi3: cp37 down to cp32) + 9 (py38 down to py30) + 1 (py3) = 19
        assert_eq!(axis.len(), 19);
        assert_eq!(axis[0], (PythonTag::CPython(3, 8), AbiTag::CPython(3, 8)));
    }
}
