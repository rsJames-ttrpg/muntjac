//! Compatible-tag list construction and ranking.

use super::tag::{AbiTag, LinuxArch, MacArch, PlatformTag, PythonTag, Tag};
use crate::config::PythonVersion;
use std::collections::HashMap;

const MANYLINUX_FLOOR_MINOR: u32 = 5;
const MUSLLINUX_FLOOR_MINOR: u32 = 0;
const MACOS_10_MIN_MINOR: u32 = 4;
const MACOS_10_MAX_MINOR: u32 = 16;

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

pub(crate) fn build_manylinux_axis(baseline: (u32, u32), arch: LinuxArch) -> Vec<PlatformTag> {
    let (major, max_minor) = baseline;
    assert!(major == 2, "manylinux baseline must have major == 2");
    let mut out = Vec::new();
    for n in (MANYLINUX_FLOOR_MINOR..=max_minor).rev() {
        out.push(PlatformTag::ManyLinux { major, minor: n, arch: arch.clone() });
    }
    out
}

pub(crate) fn build_musllinux_axis(baseline: (u32, u32), arch: LinuxArch) -> Vec<PlatformTag> {
    let (major, max_minor) = baseline;
    let mut out = Vec::new();
    for n in (MUSLLINUX_FLOOR_MINOR..=max_minor).rev() {
        out.push(PlatformTag::MuslLinux { major, minor: n, arch: arch.clone() });
    }
    out
}

pub fn build_compatible_tags(platform: &crate::config::Platform, py: PythonVersion) -> CompatibleTags {
    let python_axis = build_python_axis(py);
    let mut platform_axis = build_target_platform_axis(platform);
    platform_axis.push(PlatformTag::Any);

    // Cross product: python outer, platform inner (spec §6).
    let mut ordered = Vec::with_capacity(python_axis.len() * platform_axis.len());
    for (py_tag, abi_tag) in &python_axis {
        for plat_tag in &platform_axis {
            ordered.push(Tag {
                python: py_tag.clone(),
                abi:    abi_tag.clone(),
                plat:   plat_tag.clone(),
            });
        }
    }
    CompatibleTags::from_ordered(ordered)
}

fn build_target_platform_axis(platform: &crate::config::Platform) -> Vec<PlatformTag> {
    let target = platform.target.as_str();
    let linux_arch = if target.starts_with("x86_64") {
        LinuxArch::X86_64
    } else if target.starts_with("aarch64") {
        LinuxArch::Aarch64
    } else {
        unreachable!("Config::validate must have rejected this triple")
    };

    if target.ends_with("linux-gnu") {
        let baseline = platform.manylinux_baseline()
            .expect("validated linux-gnu platform has manylinux baseline");
        build_manylinux_axis(baseline, linux_arch)
    } else if target.ends_with("linux-musl") {
        let baseline = platform.musllinux_baseline()
            .expect("validated linux-musl platform has musllinux baseline");
        build_musllinux_axis(baseline, linux_arch)
    } else if target.ends_with("apple-darwin") {
        let macos_min = platform.macos_min_parsed()
            .expect("validated macOS platform has macos_min");
        let primary = if target.starts_with("x86_64") {
            MacArch::X86_64
        } else {
            MacArch::Arm64
        };
        build_macos_axis(macos_min, primary)
    } else {
        unreachable!("Config::validate must have rejected this triple")
    }
}

pub(crate) fn build_macos_axis(macos_min: (u32, u32), primary: MacArch) -> Vec<PlatformTag> {
    let (min_major, min_minor) = macos_min;
    let mut out = Vec::new();

    if min_major >= 11 {
        // First entries: the exact macos_min target (most-preferred), primary arch + universal2.
        out.push(PlatformTag::MacOs { major: min_major, minor: min_minor, arch: primary.clone() });
        out.push(PlatformTag::MacOs { major: min_major, minor: min_minor, arch: MacArch::Universal2 });
        // Then majors below min_major down to 11 (with minor=0).
        for m in (11..min_major).rev() {
            out.push(PlatformTag::MacOs { major: m, minor: 0, arch: primary.clone() });
            out.push(PlatformTag::MacOs { major: m, minor: 0, arch: MacArch::Universal2 });
        }
        // 10.x range: from 10.16 down to 10.4.
        for n in (MACOS_10_MIN_MINOR..=MACOS_10_MAX_MINOR).rev() {
            out.push(PlatformTag::MacOs { major: 10, minor: n, arch: primary.clone() });
            out.push(PlatformTag::MacOs { major: 10, minor: n, arch: MacArch::Universal2 });
        }
    } else {
        // min_major == 10: only the 10.x range from min_minor down to 4.
        assert!(min_major == 10, "macos_min major must be >= 10");
        for n in (MACOS_10_MIN_MINOR..=min_minor).rev() {
            out.push(PlatformTag::MacOs { major: 10, minor: n, arch: primary.clone() });
            out.push(PlatformTag::MacOs { major: 10, minor: n, arch: MacArch::Universal2 });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn manylinux_axis_2_28_x86_64() {
        let axis = build_manylinux_axis((2, 28), LinuxArch::X86_64);
        // 2_28 down to 2_5 = 24 entries.
        assert_eq!(axis.len(), 24);
        assert_eq!(axis[0], PlatformTag::ManyLinux { major: 2, minor: 28, arch: LinuxArch::X86_64 });
        assert_eq!(axis[23], PlatformTag::ManyLinux { major: 2, minor: 5, arch: LinuxArch::X86_64 });
    }

    #[test]
    fn musllinux_axis_1_2_aarch64() {
        let axis = build_musllinux_axis((1, 2), LinuxArch::Aarch64);
        // 1_2 down to 1_0 = 3 entries.
        assert_eq!(axis.len(), 3);
        assert_eq!(axis[0], PlatformTag::MuslLinux { major: 1, minor: 2, arch: LinuxArch::Aarch64 });
        assert_eq!(axis[2], PlatformTag::MuslLinux { major: 1, minor: 0, arch: LinuxArch::Aarch64 });
    }

    #[test]
    fn macos_axis_11_0_arm64() {
        let axis = build_macos_axis((11, 0), MacArch::Arm64);
        // First entry: 11_0 arm64.
        assert_eq!(axis[0], PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Arm64 });
        assert_eq!(axis[1], PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Universal2 });
        assert_eq!(axis[2], PlatformTag::MacOs { major: 10, minor: 16, arch: MacArch::Arm64 });
        // 10.16 down to 10.4 = 13 minors × 2 arches = 26.  Plus macos_min entry × 2 arches = 28.
        assert_eq!(axis.len(), 28);
        // No entries above 11_0.
        assert!(!axis.iter().any(|t| matches!(t,
            PlatformTag::MacOs { major: m, .. } if *m > 11)));
    }

    #[test]
    fn macos_axis_14_0_x86_64() {
        let axis = build_macos_axis((14, 0), MacArch::X86_64);
        // 14_0, 13_0, 12_0, 11_0 each × 2 arches = 8 entries
        // 10.16 down to 10.4 × 2 arches = 26 entries
        // Total = 34
        assert_eq!(axis.len(), 34);
        assert_eq!(axis[0], PlatformTag::MacOs { major: 14, minor: 0, arch: MacArch::X86_64 });
        assert_eq!(axis[2], PlatformTag::MacOs { major: 13, minor: 0, arch: MacArch::X86_64 });
    }

    #[test]
    fn macos_axis_10_15_arm64_only_10x_range() {
        let axis = build_macos_axis((10, 15), MacArch::Arm64);
        // 10.15 down to 10.4 = 12 minors × 2 arches = 24.
        assert_eq!(axis.len(), 24);
        assert!(axis.iter().all(|t| matches!(t,
            PlatformTag::MacOs { major: 10, .. })));
    }

    #[test]
    fn build_compatible_tags_linux_gnu_3_12() {
        use crate::config::{Platform, PythonVersion};
        let platform = Platform {
            target: "x86_64-unknown-linux-gnu".into(),
            manylinux: Some("2_28".into()),
            musllinux: None,
            macos_min: None,
        };
        let compat = build_compatible_tags(&platform, PythonVersion(3, 12));

        // Most-preferred entry: cp312-cp312-manylinux_2_28_x86_64.
        assert_eq!(compat.ordered()[0], Tag {
            python: PythonTag::CPython(3, 12),
            abi:    AbiTag::CPython(3, 12),
            plat:   PlatformTag::ManyLinux { major: 2, minor: 28, arch: LinuxArch::X86_64 },
        });
        // Final entry is always py3-none-any.
        assert_eq!(compat.ordered().last(), Some(&Tag {
            python: PythonTag::Py(3, None),
            abi:    AbiTag::None,
            plat:   PlatformTag::Any,
        }));

        let py3_manylinux = Tag {
            python: PythonTag::Py(3, None),
            abi:    AbiTag::None,
            plat:   PlatformTag::ManyLinux { major: 2, minor: 28, arch: LinuxArch::X86_64 },
        };
        let py3_any = Tag {
            python: PythonTag::Py(3, None),
            abi:    AbiTag::None,
            plat:   PlatformTag::Any,
        };
        assert!(compat.rank_of(&py3_manylinux).unwrap() < compat.rank_of(&py3_any).unwrap());
    }

    #[test]
    fn build_compatible_tags_macos_arm64_3_12() {
        use crate::config::{Platform, PythonVersion};
        let platform = Platform {
            target: "aarch64-apple-darwin".into(),
            manylinux: None,
            musllinux: None,
            macos_min: Some("11.0".into()),
        };
        let compat = build_compatible_tags(&platform, PythonVersion(3, 12));

        // Rejects wheels for macOS 12+ (above deployment target).
        let mac_12 = Tag {
            python: PythonTag::CPython(3, 12),
            abi:    AbiTag::CPython(3, 12),
            plat:   PlatformTag::MacOs { major: 12, minor: 0, arch: MacArch::Arm64 },
        };
        assert_eq!(compat.rank_of(&mac_12), None);

        // Accepts deployment target == macos_min.
        let mac_11 = Tag {
            python: PythonTag::CPython(3, 12),
            abi:    AbiTag::CPython(3, 12),
            plat:   PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Arm64 },
        };
        assert!(compat.rank_of(&mac_11).is_some());

        // arm64 platform does NOT accept x86_64-tagged wheels.
        let mac_11_x86 = Tag {
            python: PythonTag::CPython(3, 12),
            abi:    AbiTag::CPython(3, 12),
            plat:   PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::X86_64 },
        };
        assert_eq!(compat.rank_of(&mac_11_x86), None);

        // universal2 IS accepted on arm64 platforms.
        let mac_11_universal = Tag {
            python: PythonTag::CPython(3, 12),
            abi:    AbiTag::CPython(3, 12),
            plat:   PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Universal2 },
        };
        assert!(compat.rank_of(&mac_11_universal).is_some());
    }

    #[test]
    fn build_compatible_tags_linux_musl_3_11() {
        use crate::config::{Platform, PythonVersion};
        let platform = Platform {
            target: "x86_64-unknown-linux-musl".into(),
            manylinux: None,
            musllinux: Some("1_2".into()),
            macos_min: None,
        };
        let compat = build_compatible_tags(&platform, PythonVersion(3, 11));

        let m_12 = Tag {
            python: PythonTag::CPython(3, 11),
            abi:    AbiTag::CPython(3, 11),
            plat:   PlatformTag::MuslLinux { major: 1, minor: 2, arch: LinuxArch::X86_64 },
        };
        let m_11 = Tag {
            python: PythonTag::CPython(3, 11),
            abi:    AbiTag::CPython(3, 11),
            plat:   PlatformTag::MuslLinux { major: 1, minor: 1, arch: LinuxArch::X86_64 },
        };
        let m_13 = Tag {
            python: PythonTag::CPython(3, 11),
            abi:    AbiTag::CPython(3, 11),
            plat:   PlatformTag::MuslLinux { major: 1, minor: 3, arch: LinuxArch::X86_64 },
        };
        assert!(compat.rank_of(&m_12).unwrap() < compat.rank_of(&m_11).unwrap());
        assert_eq!(compat.rank_of(&m_13), None);
    }
}
