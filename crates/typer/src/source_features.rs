//! `-Xsource-features:<features>` — scalac 2.13.13+'s second migration axis.
//!
//! `-Xsource:3` is the *warning* axis: it reports where Scala 3 would behave
//! differently and (in 2.13.16) reports those migration messages as errors.
//! `-Xsource-features` is the *behaviour* axis: it makes individual Scala 3
//! behaviours actually take effect. The two are not independent — nsc gates
//! every feature on `isScala3`:
//!
//! ```text
//! // scala/tools/nsc/Global.scala, 2.13.16
//! def caseApplyCopyAccess = isScala3 && contains(o.caseApplyCopyAccess)
//! ```
//!
//! so `-Xsource-features:…` on its own is *ignored*, with a warning from
//! `ScalaSettings.conflictWarning`:
//!
//! ```text
//! $ scalac -Xsource-features:case-apply-copy-access A.scala
//! warning: Conflicting compiler settings were detected. Some settings will be ignored.
//! -Xsource-features requires -Xsource:3
//! ```
//!
//! `-Xsource:3-cross` is exactly `-Xsource:3 -Xsource-features:_`
//! (`ScalaSettings.source`'s post-set hook calls `XsourceFeatures.tryToSet(List("_"))`).
//!
//! The feature names, the `[bin]` marks and the `v2.13.x` groups below are
//! taken verbatim from `scalac -Xsource-features:help` on 2.13.16. Of these,
//! this compiler implements `case-apply-copy-access`; the rest are accepted
//! (so that `-Xsource:3-cross` keeps working and a build's flags do not have
//! to be edited) but warn when named explicitly. See `docs/not-implemented.md`.

/// One `-Xsource-features` choice, in `scalac -Xsource-features:help` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFeature {
    CaseApplyCopyAccess,
    CaseCompanionFunction,
    CaseCopyByName,
    InferOverride,
    Any2StringAdd,
    UnicodeEscapesRaw,
    StringContextScope,
    LeadingInfix,
    PackagePrefixImplicits,
    ImplicitResolution,
    DoubleDefinitions,
}

impl SourceFeature {
    /// The spelling accepted on the command line.
    pub fn name(self) -> &'static str {
        FEATURES[self as usize].1
    }

    fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

/// `(feature, spelling, help text)`, in nsc's order.
const FEATURES: &[(SourceFeature, &str, &str)] = &[
    (
        SourceFeature::CaseApplyCopyAccess,
        "case-apply-copy-access",
        "Constructor modifiers are used for apply / copy methods of case classes. [bin]",
    ),
    (
        SourceFeature::CaseCompanionFunction,
        "case-companion-function",
        "Synthetic case companion objects no longer extend FunctionN. [bin]",
    ),
    (
        SourceFeature::CaseCopyByName,
        "case-copy-by-name",
        "Synthesize case copy method with by-name parameters. [bin]",
    ),
    (
        SourceFeature::InferOverride,
        "infer-override",
        "Inferred type of member uses type of overridden member. [bin]",
    ),
    (
        SourceFeature::Any2StringAdd,
        "any2stringadd",
        "Implicit `any2stringadd` is never inferred.",
    ),
    (
        SourceFeature::UnicodeEscapesRaw,
        "unicode-escapes-raw",
        "Don't process unicode escapes in triple quoted strings and raw interpolations.",
    ),
    (
        SourceFeature::StringContextScope,
        "string-context-scope",
        "String interpolations always desugar to scala.StringContext.",
    ),
    (
        SourceFeature::LeadingInfix,
        "leading-infix",
        "Leading infix operators continue the previous line.",
    ),
    (
        SourceFeature::PackagePrefixImplicits,
        "package-prefix-implicits",
        "The package prefix p is no longer part of the implicit search scope for type p.A.",
    ),
    (
        SourceFeature::ImplicitResolution,
        "implicit-resolution",
        "Use Scala-3-style downwards comparisons for implicit search and overloading resolution (see github.com/scala/scala/pull/6037).",
    ),
    (
        SourceFeature::DoubleDefinitions,
        "double-definitions",
        "Correctly disallow double definitions differing in empty parens.",
    ),
];

/// The `v2.13.13` group, which the later groups extend. `case-copy-by-name` is
/// deliberately in none of them (nsc's help text lists it separately).
const V_2_13_13: &[SourceFeature] = &[
    SourceFeature::CaseApplyCopyAccess,
    SourceFeature::CaseCompanionFunction,
    SourceFeature::InferOverride,
    SourceFeature::Any2StringAdd,
    SourceFeature::UnicodeEscapesRaw,
    SourceFeature::StringContextScope,
    SourceFeature::LeadingInfix,
    SourceFeature::PackagePrefixImplicits,
];

/// The features this compiler actually implements. Everything else is parsed
/// and remembered, but changes nothing.
const IMPLEMENTED: &[SourceFeature] = &[SourceFeature::CaseApplyCopyAccess];

/// A set of `-Xsource-features` choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceFeatures {
    bits: u32,
}

/// What `SourceFeatures::parse` extracted from one `-Xsource-features` value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedFeatures {
    /// The resulting set.
    pub features: SourceFeatures,
    /// Features named one by one on the command line that this compiler does
    /// not implement. Naming a *group* (`_`, `v2.13.14`) does not fill this in:
    /// `-Xsource:3-cross` expands to `_`, and warning for each of the ten
    /// unimplemented members would drown the real output.
    pub unimplemented: Vec<&'static str>,
    /// `-Xsource-features:help` was asked for.
    pub help: bool,
}

impl SourceFeatures {
    /// Every feature — nsc's `_`, and what `-Xsource:3-cross` sets.
    pub fn all() -> SourceFeatures {
        let mut s = SourceFeatures::default();
        for (f, _, _) in FEATURES {
            s.bits |= f.bit();
        }
        s
    }

    pub fn is_empty(self) -> bool {
        self.bits == 0
    }

    /// The enabled features' command-line spellings, in nsc's own order.
    pub fn names(self) -> Vec<&'static str> {
        FEATURES
            .iter()
            .filter(|(f, _, _)| self.contains(*f))
            .map(|(_, n, _)| *n)
            .collect()
    }

    pub fn contains(self, f: SourceFeature) -> bool {
        self.bits & f.bit() != 0
    }

    /// `case-apply-copy-access`: the primary constructor's access modifier is
    /// copied onto the synthetic `apply` and `copy`.
    pub fn case_apply_copy_access(self) -> bool {
        self.contains(SourceFeature::CaseApplyCopyAccess)
    }

    fn insert(&mut self, f: SourceFeature) {
        self.bits |= f.bit();
    }

    fn remove(&mut self, f: SourceFeature) {
        self.bits &= !f.bit();
    }

    /// Parse one `-Xsource-features` value: a comma-separated list of feature
    /// names, group names (`_`, `v2.13.13`, `v2.13.14`, `v2.13.15`) and
    /// removals (`-case-companion-function`), or `help`.
    ///
    /// The error text is nsc's, so a mistyped flag reads the same in both
    /// compilers.
    pub fn parse(spec: &str) -> Result<ParsedFeatures, String> {
        let mut out = ParsedFeatures::default();
        if spec.trim().is_empty() {
            return Err("option -Xsource-features: requires a feature name".into());
        }
        for raw in spec.split(',') {
            let item = raw.trim();
            if item.is_empty() {
                continue;
            }
            if item == "help" {
                out.help = true;
                continue;
            }
            let (negated, name) = match item.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, item),
            };
            let group = expand_group(name);
            let is_group = group.is_some();
            let members: Vec<SourceFeature> = match group {
                Some(g) => g,
                None => match lookup(name) {
                    Some(f) => vec![f],
                    None => {
                        return Err(format!(
                            "'{name}' is not a valid choice for '-Xsource-features'"
                        ))
                    }
                },
            };
            for f in members {
                if negated {
                    out.features.remove(f);
                } else {
                    out.features.insert(f);
                    if !is_group && !IMPLEMENTED.contains(&f) {
                        let n = f.name();
                        if !out.unimplemented.contains(&n) {
                            out.unimplemented.push(n);
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// `-Xsource-features:help`, abridged to the parts that are true here.
    pub fn help_text() -> String {
        let mut s = String::new();
        s.push_str("Enable Scala 3 features under -Xsource:3.\n\n");
        s.push_str("Features can also be removed from a feature group by prefixing with `-`;\n");
        s.push_str("for example, `-Xsource-features:v2.13.14,-case-companion-function`.\n\n");
        s.push_str("`-Xsource:3-cross` is a shorthand for `-Xsource:3 -Xsource-features:_`.\n\n");
        s.push_str("Features marked with [bin] affect the binary encoding.\n");
        s.push_str("Features marked with [scala-rs] are implemented by this compiler; the\n");
        s.push_str("others are accepted and ignored (see docs/not-implemented.md).\n\n");
        s.push_str("Available features:\n\n");
        let width = FEATURES.iter().map(|(_, n, _)| n.len()).max().unwrap_or(0);
        for (f, name, help) in FEATURES {
            let mark = if IMPLEMENTED.contains(f) {
                " [scala-rs]"
            } else {
                ""
            };
            s.push_str(&format!("  {name:width$}  {help}{mark}\n"));
        }
        s.push_str(&format!(
            "  {:width$}  {}\n",
            "v2.13.13",
            V_2_13_13
                .iter()
                .map(|f| f.name())
                .collect::<Vec<_>>()
                .join(",")
        ));
        s.push_str(&format!(
            "  {:width$}  v2.13.13 plus implicit-resolution\n",
            "v2.13.14"
        ));
        s.push_str(&format!(
            "  {:width$}  v2.13.14 plus double-definitions\n",
            "v2.13.15"
        ));
        s
    }
}

fn lookup(name: &str) -> Option<SourceFeature> {
    FEATURES
        .iter()
        .find(|(_, n, _)| *n == name)
        .map(|(f, _, _)| *f)
}

/// `_` and the `v2.13.x` cumulative groups.
fn expand_group(name: &str) -> Option<Vec<SourceFeature>> {
    match name {
        "_" => Some(FEATURES.iter().map(|(f, _, _)| *f).collect()),
        "v2.13.13" => Some(V_2_13_13.to_vec()),
        "v2.13.14" => {
            let mut v = V_2_13_13.to_vec();
            v.push(SourceFeature::ImplicitResolution);
            Some(v)
        }
        "v2.13.15" => {
            let mut v = V_2_13_13.to_vec();
            v.push(SourceFeature::ImplicitResolution);
            v.push(SourceFeature::DoubleDefinitions);
            Some(v)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_feature() {
        let p = SourceFeatures::parse("case-apply-copy-access").unwrap();
        assert!(p.features.case_apply_copy_access());
        assert!(p.unimplemented.is_empty());
        assert!(!p.help);
    }

    #[test]
    fn an_unknown_feature_is_nsc_s_error() {
        let e = SourceFeatures::parse("bogus").unwrap_err();
        assert_eq!(e, "'bogus' is not a valid choice for '-Xsource-features'");
    }

    #[test]
    fn underscore_is_every_feature_and_warns_about_none() {
        let p = SourceFeatures::parse("_").unwrap();
        assert_eq!(p.features, SourceFeatures::all());
        assert!(p.unimplemented.is_empty());
    }

    #[test]
    fn a_group_can_have_a_member_removed() {
        let p = SourceFeatures::parse("v2.13.14,-case-apply-copy-access").unwrap();
        assert!(!p.features.case_apply_copy_access());
        assert!(p.features.contains(SourceFeature::ImplicitResolution));
        // `case-copy-by-name` is in no group.
        assert!(!p.features.contains(SourceFeature::CaseCopyByName));
    }

    #[test]
    fn naming_an_unimplemented_feature_is_reported() {
        let p = SourceFeatures::parse("case-apply-copy-access,leading-infix").unwrap();
        assert_eq!(p.unimplemented, vec!["leading-infix"]);
    }

    #[test]
    fn help_is_a_choice_like_nsc() {
        let p = SourceFeatures::parse("help").unwrap();
        assert!(p.help);
        assert!(SourceFeatures::help_text().contains("case-apply-copy-access"));
    }
}
