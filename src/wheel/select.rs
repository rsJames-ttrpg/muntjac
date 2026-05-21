//! Wheel selection by min-rank lookup against a CompatibleTags list.

use super::compat::CompatibleTags;
use super::tag::{Tag, parse_filename};
use crate::lock::types::Wheel;

#[derive(Debug, Clone)]
pub enum PickResult<'a> {
    Picked {
        wheel: &'a Wheel,
        matched_tag: Tag,
        rank: usize,
    },
    NoWheel,
}

pub fn pick_wheel<'a>(wheels: &'a [Wheel], compat: &CompatibleTags) -> PickResult<'a> {
    // Sort by filename for stable tie-break — removes dependence on uv.lock emission order.
    let mut sorted: Vec<&Wheel> = wheels.iter().collect();
    sorted.sort_by(|a, b| a.filename.cmp(&b.filename));

    let mut best: Option<(usize, Tag, &Wheel)> = None;

    for wheel in sorted {
        let parsed = match parse_filename(&wheel.filename) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for tag in &parsed.tags {
            if let Some(rank) = compat.rank_of(tag) {
                let improves = best.as_ref().is_none_or(|(r, _, _)| rank < *r);
                if improves {
                    best = Some((rank, tag.clone(), wheel));
                }
                #[cfg(debug_assertions)]
                {
                    // Sanity check: equal-rank tie across different wheels shouldn't happen
                    // in practice. A wheel can't list the same Tag twice, and uv.lock shouldn't
                    // emit two wheels with identical tag triples.
                    if let Some((r, _, w_prev)) = best.as_ref() {
                        if *r == rank && !std::ptr::eq(*w_prev, wheel) {
                            debug_assert_ne!(
                                w_prev.filename, wheel.filename,
                                "two wheels share best rank {} — uv.lock duplicates?",
                                rank
                            );
                        }
                    }
                }
            }
        }
    }

    match best {
        Some((rank, matched_tag, wheel)) => PickResult::Picked {
            wheel,
            matched_tag,
            rank,
        },
        None => PickResult::NoWheel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Platform, PythonVersion};
    use crate::wheel::compat::build_compatible_tags;
    use url::Url;

    fn make_wheel(filename: &str) -> Wheel {
        Wheel {
            url: Url::parse(&format!("https://example.com/{filename}")).unwrap(),
            hash: "sha256:abc".into(),
            size: None,
            filename: filename.into(),
        }
    }

    fn linux_gnu_x86() -> Platform {
        Platform {
            target: "x86_64-unknown-linux-gnu".into(),
            manylinux: Some("2_28".into()),
            musllinux: None,
            macos_min: None,
        }
    }

    #[test]
    fn picks_most_specific_wheel() {
        let wheels = vec![
            make_wheel("foo-1.0-py3-none-any.whl"),
            make_wheel("foo-1.0-cp312-abi3-manylinux_2_17_x86_64.whl"),
            make_wheel("foo-1.0-cp312-cp312-manylinux_2_17_x86_64.whl"),
        ];
        let compat = build_compatible_tags(&linux_gnu_x86(), PythonVersion(3, 12));
        let pick = pick_wheel(&wheels, &compat);
        match pick {
            PickResult::Picked { wheel, .. } => assert_eq!(
                wheel.filename,
                "foo-1.0-cp312-cp312-manylinux_2_17_x86_64.whl"
            ),
            _ => panic!("expected Picked"),
        }
    }

    #[test]
    fn returns_no_wheel_when_nothing_matches() {
        let wheels = vec![make_wheel("foo-1.0-cp310-cp310-manylinux_2_17_x86_64.whl")];
        let compat = build_compatible_tags(&linux_gnu_x86(), PythonVersion(3, 12));
        assert!(matches!(pick_wheel(&wheels, &compat), PickResult::NoWheel));
    }

    #[test]
    fn unparseable_filenames_are_skipped() {
        let wheels = vec![
            make_wheel("garbage.whl"),
            make_wheel("foo-1.0-py3-none-any.whl"),
        ];
        let compat = build_compatible_tags(&linux_gnu_x86(), PythonVersion(3, 12));
        match pick_wheel(&wheels, &compat) {
            PickResult::Picked { wheel, .. } => {
                assert_eq!(wheel.filename, "foo-1.0-py3-none-any.whl")
            }
            _ => panic!("expected Picked"),
        }
    }

    #[test]
    fn input_order_does_not_affect_result() {
        let w1 = make_wheel("foo-1.0-cp312-cp312-manylinux_2_17_x86_64.whl");
        let w2 = make_wheel("foo-1.0-py3-none-any.whl");
        let compat = build_compatible_tags(&linux_gnu_x86(), PythonVersion(3, 12));
        let order_a = [w1.clone(), w2.clone()];
        let order_b = [w2, w1];
        let pick_a = pick_wheel(&order_a, &compat);
        let pick_b = pick_wheel(&order_b, &compat);
        match (pick_a, pick_b) {
            (PickResult::Picked { wheel: a, .. }, PickResult::Picked { wheel: b, .. }) => {
                assert_eq!(a.filename, b.filename)
            }
            _ => panic!("expected both Picked"),
        }
    }
}
