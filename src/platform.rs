//! PEP 508 marker environment construction and evaluation.
//!
//! Maps muntjac's `Platform` + `PythonVersion` to the env strings that
//! `pep508_rs::MarkerEnvironment` expects.

use pep508_rs::{MarkerEnvironment, MarkerEnvironmentBuilder, MarkerTree};

use crate::config::{Platform, PythonVersion};

// ---- Public API ----

pub fn marker_env(platform: &Platform, python: PythonVersion) -> MarkerEnvironment {
    let env = derive_env_strings(&platform.target);
    let full = format!("{}.{}.0", python.0, python.1);
    let short = format!("{}.{}", python.0, python.1);

    MarkerEnvironment::try_from(MarkerEnvironmentBuilder {
        implementation_name: "cpython",
        implementation_version: &full,
        os_name: env.os_name,
        platform_machine: env.platform_machine,
        platform_python_implementation: "CPython",
        platform_release: "",
        platform_system: env.platform_system,
        platform_version: "",
        python_full_version: &full,
        python_version: &short,
        sys_platform: env.sys_platform,
    })
    .expect("constructed MarkerEnvironment values are valid")
}

pub fn marker_matches(marker: Option<&MarkerTree>, env: &MarkerEnvironment) -> bool {
    match marker {
        None => true,
        Some(m) => m.evaluate(env, &[]),
    }
}

struct EnvStrings {
    os_name: &'static str,
    sys_platform: &'static str,
    platform_system: &'static str,
    platform_machine: &'static str,
}

fn derive_env_strings(target: &str) -> EnvStrings {
    match target {
        "x86_64-unknown-linux-gnu" | "x86_64-unknown-linux-musl" => EnvStrings {
            os_name: "posix",
            sys_platform: "linux",
            platform_system: "Linux",
            platform_machine: "x86_64",
        },
        "aarch64-unknown-linux-gnu" | "aarch64-unknown-linux-musl" => EnvStrings {
            os_name: "posix",
            sys_platform: "linux",
            platform_system: "Linux",
            platform_machine: "aarch64",
        },
        "x86_64-apple-darwin" => EnvStrings {
            os_name: "posix",
            sys_platform: "darwin",
            platform_system: "Darwin",
            platform_machine: "x86_64",
        },
        "aarch64-apple-darwin" => EnvStrings {
            os_name: "posix",
            sys_platform: "darwin",
            platform_system: "Darwin",
            platform_machine: "arm64",
        },
        other => {
            eprintln!("warning: unknown target triple `{other}`; marker eval may be inaccurate");
            EnvStrings {
                os_name: "",
                sys_platform: "",
                platform_system: "",
                platform_machine: "",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn linux_x86_64() -> Platform {
        Platform {
            target: "x86_64-unknown-linux-gnu".into(),
            manylinux: Some("2_17".into()),
            musllinux: None,
            macos_min: None,
        }
    }

    fn macos_arm64() -> Platform {
        Platform {
            target: "aarch64-apple-darwin".into(),
            manylinux: None,
            musllinux: None,
            macos_min: Some("11.0".into()),
        }
    }

    fn marker(s: &str) -> MarkerTree {
        MarkerTree::from_str(s).expect("marker parse")
    }

    #[test]
    fn python_version_marker() {
        let env = marker_env(&linux_x86_64(), PythonVersion(3, 11));
        assert!(marker_matches(
            Some(&marker("python_version >= '3.10'")),
            &env
        ));
        assert!(marker_matches(
            Some(&marker("python_version < '3.13'")),
            &env
        ));
        assert!(!marker_matches(
            Some(&marker("python_version < '3.11'")),
            &env
        ));
    }

    #[test]
    fn sys_platform_marker() {
        let linux = marker_env(&linux_x86_64(), PythonVersion(3, 12));
        let mac = marker_env(&macos_arm64(), PythonVersion(3, 12));
        let m = marker("sys_platform == 'linux'");
        assert!(marker_matches(Some(&m), &linux));
        assert!(!marker_matches(Some(&m), &mac));
    }

    #[test]
    fn platform_machine_marker() {
        let linux_x86 = marker_env(&linux_x86_64(), PythonVersion(3, 12));
        let macos_arm = marker_env(&macos_arm64(), PythonVersion(3, 12));
        assert!(marker_matches(
            Some(&marker("platform_machine == 'x86_64'")),
            &linux_x86
        ));
        assert!(marker_matches(
            Some(&marker("platform_machine == 'arm64'")),
            &macos_arm
        ));
        assert!(!marker_matches(
            Some(&marker("platform_machine == 'arm64'")),
            &linux_x86
        ));
    }

    #[test]
    fn no_marker_always_matches() {
        let env = marker_env(&linux_x86_64(), PythonVersion(3, 11));
        assert!(marker_matches(None, &env));
    }

    #[test]
    fn full_version_lowest_patch() {
        let env = marker_env(&linux_x86_64(), PythonVersion(3, 12));
        assert!(marker_matches(
            Some(&marker("python_full_version >= '3.12.0'")),
            &env
        ));
        assert!(!marker_matches(
            Some(&marker("python_full_version >= '3.12.4'")),
            &env
        ));
    }

    #[test]
    fn excludes_emscripten() {
        let env = marker_env(&linux_x86_64(), PythonVersion(3, 12));
        assert!(marker_matches(
            Some(&marker("platform_system != 'Emscripten'")),
            &env
        ));
    }
}
