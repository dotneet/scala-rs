//! nsc's reporting layer: the order warnings come out in, which of them are
//! summarized instead of printed, the duplicate filter, `-Werror`, and the
//! `ConsoleReporter` rendering (`file:line: warning: msg`, source line, caret,
//! and the closing `N warnings` / `N errors` counts).
//!
//! The pieces follow `scala.tools.nsc.Reporting.PerRunReporting` and
//! `scala.tools.nsc.reporters.{FilteringReporter, ConsoleReporter}` of
//! scalac 2.13.16:
//!
//! * warnings issued before a unit's typer finishes (parser and typer) are
//!   *suspended* and released, in issue order, when that unit's typer is done
//!   (`reportSuspendedMessages`), so they come out unit by unit ahead of every
//!   warning of a later phase; later phases report as they go, unit by unit.
//! * `cat=deprecation` and `cat=feature` default to `WarningSummary`: they are
//!   counted per `since` version and reported at the end of a run without
//!   errors as `N deprecations (since X); re-run with -deprecation for details`.
//! * the filtering reporter drops a warning at a point that already carries an
//!   error, or the same (prefix) message.
//! * `-Werror` keeps the warnings as warnings and adds one position-less
//!   `No warnings can be incurred under -Werror.` error after the summaries.

use crate::{Diagnostic, Level, SourceFile, Span};

/// The nsc phase that issues a diagnostic. Only the relative order matters:
/// within a run without errors, diagnostics are reported phase by phase, and
/// inside a phase unit by unit (command-line order).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// The scanner and parser: suspended with the typer's warnings, and
    /// issued before them.
    Parser,
    /// `typer` (its warnings and the parser's are suspended until the unit's
    /// typer finishes, so the two are released unit by unit).
    #[default]
    Typer,
    Refchecks,
    Patmat,
    Uncurry,
    Specialize,
    ExplicitOuter,
    Erasure,
    Constructors,
    /// Position-less diagnostics issued at the end of the run.
    Summary,
}

/// nsc's `WarningCategory`, reduced to what reporting treats differently.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum WarnCategory {
    #[default]
    Other,
    /// `cat=deprecation`; `since` is the `@deprecated` annotation's version
    /// string, empty when it has none.
    Deprecation { since: String },
    /// `cat=feature` (every `feature-*` sub-category summarizes as `feature`).
    Feature,
}

/// The reporting settings of a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WarnSettings {
    /// `-deprecation`: print each deprecation instead of the summary.
    pub deprecation: bool,
    /// `-feature`: print each feature warning instead of the summary.
    pub feature: bool,
    /// `-nowarn`: silence every warning.
    pub nowarn: bool,
    /// `-Werror` / `-Xfatal-warnings`.
    pub fatal_warnings: bool,
}

/// Turn the diagnostics a run collected into the ones nsc reports, in the
/// order it reports them. See the module documentation.
pub fn finish_diagnostics(diags: Vec<Diagnostic>, settings: &WarnSettings) -> Vec<Diagnostic> {
    let has_errors = diags.iter().any(|d| d.level == Level::Error);
    let mut diags = diags;
    if !has_errors {
        // A run with errors stops after the phase that reported them, and
        // nothing of a later phase exists; the order of what is there is the
        // order it was issued in. Without errors every phase ran, and the
        // suspension described above decides the order.
        diags.sort_by_key(|d| match d.phase {
            Phase::Summary => (d.phase, usize::MAX, d.phase),
            // Suspended until the unit's typer is done: per unit, the
            // parser's warnings, then the typer's.
            Phase::Parser | Phase::Typer => (Phase::Typer, d.file_index, d.phase),
            _ => (d.phase, d.file_index, d.phase),
        });
    }
    let mut out = Vec::with_capacity(diags.len());
    // Summarized warnings, keyed by (category name, since) in issue order.
    let mut deprecations: Vec<(String, (usize, Span))> = Vec::new();
    let mut features: Vec<(usize, Span)> = Vec::new();
    // FilteringReporter.duplicateOk: per point, the highest severity shown so
    // far and the messages shown there.
    let mut shown: std::collections::HashMap<(usize, u32), (Level, Vec<String>)> =
        std::collections::HashMap::new();
    for d in diags {
        if d.level == Level::Warning {
            if settings.nowarn {
                continue;
            }
            match &d.category {
                WarnCategory::Deprecation { since } if !settings.deprecation => {
                    let key = (d.file_index, d.span);
                    if !deprecations.iter().any(|(_, k)| *k == key) {
                        deprecations.push((since.clone(), key));
                    }
                    continue;
                }
                WarnCategory::Feature if !settings.feature => {
                    let key = (d.file_index, d.span);
                    if !features.contains(&key) {
                        features.push(key);
                    }
                    continue;
                }
                _ => {}
            }
        }
        if d.span.is_dummy() || d.file_index == usize::MAX {
            out.push(d);
            continue;
        }
        let key = (d.file_index, d.span.lo.0);
        match d.level {
            // Errors are never filtered here: nsc would also drop a second
            // error at the same point, but which of two errors comes first is
            // an accident of our checking order, so both are kept.
            Level::Error => {
                let e = shown.entry(key).or_insert((Level::Error, Vec::new()));
                e.0 = Level::Error;
                e.1.push(d.message.clone());
                out.push(d);
            }
            Level::Warning => {
                let show = match shown.get(&key) {
                    None => true,
                    Some((Level::Error, _)) => false,
                    Some((Level::Warning, msgs)) => !msgs.iter().any(|m| d.message.starts_with(m)),
                    Some((Level::Note, _)) => true,
                };
                if show {
                    let e = shown.entry(key).or_insert((Level::Warning, Vec::new()));
                    e.0 = Level::Warning;
                    e.1.push(d.message.clone());
                    out.push(d);
                }
            }
            Level::Note => out.push(d),
        }
    }
    let has_errors = out.iter().any(|d| d.level == Level::Error);
    if !has_errors && !settings.nowarn {
        // `summarizeErrors`: categories in name order ("deprecation" <
        // "feature"), and within deprecation one line per `since` version in
        // string order, plus an `in total` line when there is more than one.
        if !deprecations.is_empty() {
            let mut by_since: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for (since, _) in &deprecations {
                *by_since.entry(since.clone()).or_default() += 1;
            }
            let several = by_since.len() > 1;
            let rerun = "; re-run with -deprecation for details";
            for (since, n) in &by_since {
                let since_txt = if since.is_empty() {
                    String::new()
                } else {
                    format!(" (since {since})")
                };
                let count = count_elements(*n, "deprecation");
                let tail = if several { "" } else { rerun };
                out.push(Diagnostic::unpositioned(
                    Level::Warning,
                    format!("{count}{since_txt}{tail}"),
                ));
            }
            if several {
                let count = count_elements(deprecations.len(), "deprecation");
                out.push(Diagnostic::unpositioned(
                    Level::Warning,
                    format!("{count} in total{rerun}"),
                ));
            }
        }
        if !features.is_empty() {
            let count = count_elements(features.len(), "feature warning");
            out.push(Diagnostic::unpositioned(
                Level::Warning,
                format!("{count}; re-run with -feature for details"),
            ));
        }
        if settings.fatal_warnings && out.iter().any(|d| d.level == Level::Warning) {
            out.push(Diagnostic::unpositioned(
                Level::Error,
                "No warnings can be incurred under -Werror.",
            ));
        }
    }
    out
}

/// `StringOps.countElementsAsString`.
fn count_elements(n: usize, element: &str) -> String {
    match n {
        0 => format!("no {element}s"),
        1 => format!("1 {element}"),
        _ => format!("{n} {element}s"),
    }
}

/// `trimTrailingSpace`, per line (`trimAllTrailingSpace`). Java's
/// `isWhitespace` excludes the non-breaking spaces.
fn trim_all_trailing_space(s: &str) -> String {
    let is_ws = |c: char| c.is_whitespace() && !matches!(c, '\u{00A0}' | '\u{2007}' | '\u{202F}');
    s.split('\n')
        .map(|l| l.trim_end_matches(is_ws))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `Position.showError`'s escaping of the source line.
fn escape_line(s: &str) -> String {
    let uable = |c: char| ((c as u32) < 0x20 && c != '\t') || c as u32 == 0x7F;
    if !s.chars().any(uable) {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if uable(c) {
            out.push_str(&format!("\\u{:04x}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// One diagnostic the way `PrintReporter.display` prints it.
fn render_one(d: &Diagnostic, sources: &[SourceFile]) -> String {
    let label = match d.level {
        Level::Error => "error: ",
        Level::Warning => "warning: ",
        Level::Note => "",
    };
    let mut msg = format!("{label}{}", d.message);
    for n in &d.notes {
        msg.push('\n');
        msg.push_str(n);
    }
    let file = sources.get(d.file_index);
    let text = match file {
        Some(file) if !d.span.is_dummy() => {
            let (line, col) = file.loc(d.span.lo);
            let content = file.line_text(line);
            let line_start = d.span.lo.0 as usize - (col as usize - 1);
            let point = (d.span.lo.0 as usize).min(file.src.len());
            let before = file.src.get(line_start.min(point)..point).unwrap_or("");
            let mut caret = String::new();
            for c in before.chars() {
                if c == '\t' {
                    caret.push('\t');
                } else {
                    // One column per UTF-16 code unit, as `lineCaret` counts
                    // `source.content` (a `char` array).
                    for _ in 0..c.len_utf16() {
                        caret.push(' ');
                    }
                }
            }
            caret.push('^');
            format!(
                "{}:{line}: {msg}\n{}\n{caret}",
                file.name,
                escape_line(content)
            )
        }
        _ => msg,
    };
    trim_all_trailing_space(&text)
}

/// The run's diagnostics as scalac's `ConsoleReporter` prints them, with the
/// closing counts of `ConsoleReporter.finish`.
pub fn render_scalac(diags: &[Diagnostic], sources: &[SourceFile]) -> String {
    let mut s = String::new();
    for d in diags {
        s.push_str(&render_one(d, sources));
        s.push('\n');
    }
    let errors = diags.iter().filter(|d| d.level == Level::Error).count();
    let warnings = diags.iter().filter(|d| d.level == Level::Warning).count();
    if warnings > 0 {
        s.push_str(&count_elements(warnings, "warning"));
        s.push('\n');
    }
    if errors > 0 {
        s.push_str(&count_elements(errors, "error"));
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warn(file: usize, lo: u32, msg: &str) -> Diagnostic {
        Diagnostic::warning(file, Span::new(lo, lo + 1), msg)
    }

    #[test]
    fn scalac_rendering_keeps_tabs_in_the_caret_line() {
        let sf = SourceFile::new("a.scala", "object A {\n\t1  \n}\n");
        let d = warn(0, 12, "a pure expression does nothing in statement position");
        let s = render_scalac(&[d], &[sf]);
        assert_eq!(
            s,
            "a.scala:2: warning: a pure expression does nothing in statement position\n\t1\n\t^\n1 warning\n"
        );
    }

    #[test]
    fn deprecations_are_summarized_per_since_version() {
        let d = |lo, since: &str| {
            warn(0, lo, "x is deprecated").with_category(WarnCategory::Deprecation {
                since: since.to_string(),
            })
        };
        let out = finish_diagnostics(
            vec![d(1, "2.13.0"), d(3, "2.13.0")],
            &WarnSettings::default(),
        );
        let msgs: Vec<_> = out.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            msgs,
            ["2 deprecations (since 2.13.0); re-run with -deprecation for details"]
        );
        let out = finish_diagnostics(vec![d(1, "2.13.0"), d(3, "2.11.0")], &WarnSettings::default());
        let msgs: Vec<_> = out.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            msgs,
            [
                "1 deprecation (since 2.11.0)",
                "1 deprecation (since 2.13.0)",
                "2 deprecations in total; re-run with -deprecation for details"
            ]
        );
    }

    #[test]
    fn werror_adds_one_error_after_the_warnings() {
        let out = finish_diagnostics(
            vec![warn(0, 1, "w")],
            &WarnSettings {
                fatal_warnings: true,
                ..WarnSettings::default()
            },
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].level, Level::Error);
        assert_eq!(out[1].message, "No warnings can be incurred under -Werror.");
    }

    #[test]
    fn later_phases_report_after_typer_unit_by_unit() {
        let out = finish_diagnostics(
            vec![
                warn(1, 1, "patmat b").in_phase(Phase::Patmat),
                warn(0, 1, "patmat a").in_phase(Phase::Patmat),
                warn(1, 2, "typer b"),
                warn(0, 2, "typer a"),
            ],
            &WarnSettings::default(),
        );
        let msgs: Vec<_> = out.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(msgs, ["typer a", "typer b", "patmat a", "patmat b"]);
    }
}
