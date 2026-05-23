//! `cfg(<expr>)` predicate parser + evaluator.
//!
//! Grammar (informal):
//!   expr       := atom | combinator
//!   atom       := IDENT '=' STRING
//!   combinator := ('all'|'any'|'not') '(' expr (',' expr)* ')'
//!   IDENT      := version | python | target_os | target_arch | target_env

use crate::fixup::error::CfgParseError;

#[derive(Debug, Clone, PartialEq)]
pub enum CfgPredicate {
    Version(String), // PEP 440 specifier string, e.g. ">=10.0"
    Python(String),
    TargetOs(String),
    TargetArch(String),
    TargetEnv(String),
    All(Vec<CfgPredicate>),
    Any(Vec<CfgPredicate>),
    Not(Box<CfgPredicate>),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CfgContext<'a> {
    pub package_version: &'a pep440_rs::Version,
    pub python_version: pep440_rs::Version,
    pub target_os: &'a str,
    pub target_arch: &'a str,
    pub target_env: &'a str,
}

impl CfgPredicate {
    /// Parse a predicate string (the inside of `cfg(...)` — i.e. the
    /// stripped section-header content).
    pub fn parse(input: &str) -> Result<Self, CfgParseError> {
        let mut p = Parser::new(input);
        let expr = p.parse_expr()?;
        p.skip_ws();
        if p.pos < p.input.len() {
            return Err(CfgParseError::AtOffset {
                offset: p.pos,
                message: format!("unexpected trailing input: `{}`", &p.input[p.pos..]),
            });
        }
        Ok(expr)
    }
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn parse_expr(&mut self) -> Result<CfgPredicate, CfgParseError> {
        self.skip_ws();
        let ident = self.parse_ident()?;
        match ident.as_str() {
            "all" | "any" | "not" => self.parse_combinator(&ident),
            atom => self.parse_atom_body(atom),
        }
    }

    fn parse_ident(&mut self) -> Result<String, CfgParseError> {
        self.skip_ws();
        let start = self.pos;
        while self.pos < self.input.len() {
            let b = self.input.as_bytes()[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if start == self.pos {
            return Err(CfgParseError::AtOffset {
                offset: self.pos,
                message: "expected identifier".into(),
            });
        }
        Ok(self.input[start..self.pos].to_string())
    }

    fn expect_byte(&mut self, b: u8) -> Result<(), CfgParseError> {
        self.skip_ws();
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(CfgParseError::AtOffset {
                offset: self.pos,
                message: format!("expected `{}`", b as char),
            })
        }
    }

    fn parse_combinator(&mut self, name: &str) -> Result<CfgPredicate, CfgParseError> {
        self.expect_byte(b'(')?;
        let mut args = Vec::new();
        loop {
            args.push(self.parse_expr()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b')') => {
                    self.pos += 1;
                    break;
                }
                _ => {
                    return Err(CfgParseError::AtOffset {
                        offset: self.pos,
                        message: "expected `,` or `)`".into(),
                    });
                }
            }
        }
        match name {
            "all" => Ok(CfgPredicate::All(args)),
            "any" => Ok(CfgPredicate::Any(args)),
            "not" => {
                if args.len() != 1 {
                    return Err(CfgParseError::AtOffset {
                        offset: self.pos,
                        message: format!("`not` takes exactly 1 argument, got {}", args.len()),
                    });
                }
                Ok(CfgPredicate::Not(Box::new(
                    args.into_iter().next().unwrap(),
                )))
            }
            _ => unreachable!("matched above"),
        }
    }

    fn parse_atom_body(&mut self, ident: &str) -> Result<CfgPredicate, CfgParseError> {
        self.expect_byte(b'=')?;
        let value = self.parse_string()?;
        match ident {
            "version" => Ok(CfgPredicate::Version(value)),
            "python" => Ok(CfgPredicate::Python(value)),
            "target_os" => Ok(CfgPredicate::TargetOs(value)),
            "target_arch" => Ok(CfgPredicate::TargetArch(value)),
            "target_env" => Ok(CfgPredicate::TargetEnv(value)),
            other => Err(CfgParseError::AtOffset {
                offset: self.pos,
                message: format!("unknown atom `{}`", other),
            }),
        }
    }

    fn parse_string(&mut self) -> Result<String, CfgParseError> {
        self.skip_ws();
        if self.peek() != Some(b'"') {
            return Err(CfgParseError::AtOffset {
                offset: self.pos,
                message: "expected `\"`".into(),
            });
        }
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos] != b'"' {
            self.pos += 1;
        }
        if self.pos >= self.input.len() {
            return Err(CfgParseError::AtOffset {
                offset: start,
                message: "unterminated string".into(),
            });
        }
        let value = self.input[start..self.pos].to_string();
        self.pos += 1; // consume closing quote
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_target_os_atom() {
        let p = CfgPredicate::parse("target_os = \"linux\"").unwrap();
        assert_eq!(p, CfgPredicate::TargetOs("linux".into()));
    }

    #[test]
    fn parses_version_atom() {
        let p = CfgPredicate::parse("version = \">=10.0\"").unwrap();
        assert_eq!(p, CfgPredicate::Version(">=10.0".into()));
    }

    #[test]
    fn parses_all_combinator() {
        let p = CfgPredicate::parse("all(target_os = \"linux\", target_env = \"musl\")").unwrap();
        match p {
            CfgPredicate::All(args) => {
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], CfgPredicate::TargetOs("linux".into()));
                assert_eq!(args[1], CfgPredicate::TargetEnv("musl".into()));
            }
            other => panic!("expected All, got {:?}", other),
        }
    }

    #[test]
    fn parses_any_with_nested() {
        let p = CfgPredicate::parse(
            "any(target_os = \"macos\", all(target_os = \"linux\", target_arch = \"aarch64\"))",
        )
        .unwrap();
        match p {
            CfgPredicate::Any(args) => assert_eq!(args.len(), 2),
            other => panic!("expected Any, got {:?}", other),
        }
    }

    #[test]
    fn parses_not_unary() {
        let p = CfgPredicate::parse("not(target_os = \"windows\")").unwrap();
        match p {
            CfgPredicate::Not(inner) => {
                assert_eq!(*inner, CfgPredicate::TargetOs("windows".into()));
            }
            other => panic!("expected Not, got {:?}", other),
        }
    }

    #[test]
    fn rejects_not_with_two_args() {
        let err =
            CfgPredicate::parse("not(target_os = \"linux\", target_os = \"macos\")").unwrap_err();
        assert!(err.to_string().contains("not"), "got: {}", err);
    }

    #[test]
    fn rejects_unknown_atom() {
        let err = CfgPredicate::parse("target_family = \"unix\"").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("target_family"), "got: {}", msg);
    }

    #[test]
    fn rejects_trailing_input() {
        let err = CfgPredicate::parse("target_os = \"linux\" extra").unwrap_err();
        assert!(err.to_string().contains("trailing"), "got: {}", err);
    }

    #[test]
    fn parses_with_whitespace_insensitivity() {
        let p = CfgPredicate::parse("  all (  target_os = \"linux\" , python = \">=3.12\"  )  ")
            .unwrap();
        match p {
            CfgPredicate::All(args) => assert_eq!(args.len(), 2),
            other => panic!("expected All, got {:?}", other),
        }
    }

    #[test]
    fn parses_python_atom() {
        let p = CfgPredicate::parse("python = \">=3.12\"").unwrap();
        assert_eq!(p, CfgPredicate::Python(">=3.12".into()));
    }
}
