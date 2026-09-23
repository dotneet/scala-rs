//! Where a macro expansion's time goes (`SCALA_RS_MACRO_TIMING=1`), and an
//! opt-in census of repeated macro implicit queries
//! (`SCALA_RS_MACRO_IMPLICIT_STATS=1`).
//!
//! The bridge has four places a second can hide, and before this module nobody
//! had measured which: starting the engine JVM, the conversation on the pipe
//! (one line out, one line back, with the engine asking its own questions in
//! the middle), the macro implementation's own run inside the JVM, and
//! rebuilding and re-typing the tree it hands back on the Rust side. Each is
//! timed separately, per expansion and in total, and the engine reports its own
//! two numbers through a `(timing)` request so the wall time on the pipe can be
//! split into "the engine was computing" and "we were answering it".
//!
//! The instrumentation is off unless the environment variable is set: the
//! per-expansion cost is two `Instant::now()` calls plus one extra round trip,
//! which is nothing next to an expansion but is not free either.

use std::time::Duration;

use rustc_hash::FxHashMap;

#[derive(Default)]
struct ImplicitQueryCount {
    calls: u64,
    summary: String,
}

/// Diagnostic-only counts of identical `c.inferImplicitValue` searches.
///
/// This deliberately stores observations rather than answers: the fingerprint
/// is useful for deciding whether a result cache would pay, but is not yet a
/// claim that the listed fields are a sufficient cache key.
#[derive(Default)]
struct ImplicitQueryStats {
    enabled: bool,
    total: u64,
    by_fingerprint: FxHashMap<String, ImplicitQueryCount>,
}

impl ImplicitQueryStats {
    fn new() -> Self {
        Self {
            enabled: std::env::var_os("SCALA_RS_MACRO_IMPLICIT_STATS").is_some(),
            ..Default::default()
        }
    }

    fn record(&mut self, fingerprint: String, summary: String) {
        if !self.enabled {
            return;
        }
        self.total = self.total.saturating_add(1);
        let entry = self.by_fingerprint.entry(fingerprint).or_default();
        entry.calls = entry.calls.saturating_add(1);
        if entry.summary.is_empty() {
            entry.summary = summary;
        }
    }

    fn report(&self) {
        if !self.enabled {
            return;
        }
        let unique = self.by_fingerprint.len() as u64;
        let repeated = self.total.saturating_sub(unique);
        let pct = if self.total == 0 {
            0.0
        } else {
            repeated as f64 * 100.0 / self.total as f64
        };
        eprintln!(
            "[macro implicit queries] search attempts {} | unique {} | repeated {} ({pct:.1}%)",
            self.total, unique, repeated
        );
        eprintln!(
            "[macro implicit queries] context-sensitive lower bound; retries with different rejected candidates are distinct"
        );
        let mut top: Vec<(&String, &ImplicitQueryCount)> = self
            .by_fingerprint
            .iter()
            .filter(|(_, count)| count.calls > 1)
            .collect();
        top.sort_by(|(ak, a), (bk, b)| b.calls.cmp(&a.calls).then_with(|| ak.cmp(bk)));
        for (rank, (fingerprint, count)) in top.into_iter().take(20).enumerate() {
            eprintln!(
                "[macro implicit queries] #{:<2} {:>7} calls | {} | fingerprint={}",
                rank + 1,
                count.calls,
                count.summary,
                fingerprint
            );
        }
    }
}

/// One macro application's breakdown.
#[derive(Default)]
pub(crate) struct ExpansionTiming {
    /// `impl class.method` of the macro that was expanded.
    pub(crate) name: String,
    /// Serialising the request: the argument trees, the type tags and the
    /// receiver (`Typer::expansion_request`).
    pub(crate) request: Duration,
    /// Wall time between sending the request and reading the final reply.
    pub(crate) rpc: Duration,
    /// How many lines the engine wrote back before the reply (its questions
    /// and its `println` output).
    pub(crate) round_trips: u32,
    /// Per question kind: how many, and how long scala-rs took to answer.
    pub(crate) answers: Vec<(&'static str, u32, Duration)>,
    /// Rebuilding the reply into a scala-rs tree (`Typer::tree_from_reply`).
    pub(crate) rebuild: Duration,
    /// Typechecking the expansion at the call site.
    pub(crate) retype: Duration,
    /// Inside `Method.invoke` in the engine -- the implementation's own run,
    /// including the time it spent waiting for our answers.
    pub(crate) engine_invoke: Duration,
    /// The part of `engine_invoke` the engine spent blocked on our answer.
    pub(crate) engine_wait: Duration,
    /// The whole exchange inside the engine: building the argument trees and
    /// the type tags, running the implementation, serialising the reply.
    pub(crate) engine_handle: Duration,
}

impl ExpansionTiming {
    /// The time the engine was working for us: everything it did, minus what
    /// it spent blocked on our answers.
    fn engine_compute(&self) -> Duration {
        self.engine_handle.saturating_sub(self.engine_wait)
    }

    /// Of that, the macro implementation's own run.
    fn impl_run(&self) -> Duration {
        self.engine_invoke.saturating_sub(self.engine_wait)
    }

    fn answered(&self) -> Duration {
        self.answers.iter().map(|(_, _, d)| *d).sum()
    }

    fn total(&self) -> Duration {
        self.request + self.rpc + self.rebuild + self.retype
    }
}

/// The whole run's macro timing.
#[derive(Default)]
pub(crate) struct MacroTiming {
    pub(crate) enabled: bool,
    /// `start_engine`: `javac` if the cache was cold, then the JVM up to
    /// `(ready)`.
    pub(crate) engine_start: Duration,
    pub(crate) expansions: Vec<ExpansionTiming>,
    implicit_queries: ImplicitQueryStats,
}

impl MacroTiming {
    pub(crate) fn new() -> Self {
        MacroTiming {
            enabled: std::env::var_os("SCALA_RS_MACRO_TIMING").is_some(),
            implicit_queries: ImplicitQueryStats::new(),
            ..Default::default()
        }
    }

    pub(crate) fn implicit_query_stats_enabled(&self) -> bool {
        self.implicit_queries.enabled
    }

    pub(crate) fn record_implicit_query(&mut self, fingerprint: String, summary: String) {
        self.implicit_queries.record(fingerprint, summary);
    }

    /// Write the table on stderr. Markdown, so it can be pasted into
    /// `docs/performance.md` as it stands.
    pub(crate) fn report(&self) {
        if self.enabled {
            self.report_timing();
        }
        self.implicit_queries.report();
    }

    fn report_timing(&self) {
        let secs = |d: Duration| d.as_secs_f64();
        if self.expansions.is_empty() {
            eprintln!(
                "[macro timing] engine start-up {:.3} s, no expansion ran",
                secs(self.engine_start)
            );
            return;
        }
        eprintln!(
            "[macro timing] engine start-up: {:.3} s",
            secs(self.engine_start)
        );
        eprintln!(
            "[macro timing] {} expansions | {:<34} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            self.expansions.len(),
            "macro",
            "request",
            "rpc",
            "engine",
            "impl",
            "answers",
            "rebuild",
            "retype",
            "trips",
        );
        for (i, e) in self.expansions.iter().enumerate() {
            eprintln!(
                "[macro timing] #{:<3} {:<34} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8}",
                i + 1,
                e.name,
                secs(e.request),
                secs(e.rpc),
                secs(e.engine_compute()),
                secs(e.impl_run()),
                secs(e.answered()),
                secs(e.rebuild),
                secs(e.retype),
                e.round_trips,
            );
        }
        let sum = |f: fn(&ExpansionTiming) -> Duration| -> Duration {
            self.expansions.iter().map(f).sum()
        };
        eprintln!(
            "[macro timing] {:<38} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8}",
            "TOTAL",
            secs(sum(|e| e.request)),
            secs(sum(|e| e.rpc)),
            secs(sum(ExpansionTiming::engine_compute)),
            secs(sum(ExpansionTiming::impl_run)),
            secs(sum(ExpansionTiming::answered)),
            secs(sum(|e| e.rebuild)),
            secs(sum(|e| e.retype)),
            self.expansions.iter().map(|e| e.round_trips).sum::<u32>(),
        );
        // Questions are what the engine's lazy mirror is made of, so the split
        // by kind is the number that says whether an engine-side symbol cache
        // would pay.
        let mut kinds: Vec<(&'static str, u32, Duration)> = Vec::new();
        for e in &self.expansions {
            for (kind, n, d) in &e.answers {
                match kinds.iter_mut().find(|(k, _, _)| k == kind) {
                    Some(slot) => {
                        slot.1 += n;
                        slot.2 += *d;
                    }
                    None => kinds.push((kind, *n, *d)),
                }
            }
        }
        kinds.sort_by_key(|k| std::cmp::Reverse(k.2));
        for (kind, n, d) in &kinds {
            eprintln!(
                "[macro timing] question {:<24} {:>7} calls {:>8.3} s",
                kind,
                n,
                secs(*d)
            );
        }
        eprintln!(
            "[macro timing] expansion wall total {:.3} s (engine start-up included: {:.3} s)",
            secs(sum(ExpansionTiming::total)),
            secs(sum(ExpansionTiming::total) + self.engine_start),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implicit_query_stats_count_total_unique_and_repeated() {
        let mut stats = ImplicitQueryStats {
            enabled: true,
            ..Default::default()
        };
        stats.record("same".into(), "first spelling wins".into());
        stats.record("same".into(), "ignored".into());
        stats.record("other".into(), "other".into());

        assert_eq!(stats.total, 3);
        assert_eq!(stats.by_fingerprint.len(), 2);
        assert_eq!(stats.by_fingerprint["same"].calls, 2);
        assert_eq!(stats.by_fingerprint["same"].summary, "first spelling wins");
    }

    #[test]
    fn disabled_implicit_query_stats_do_not_retain_observations() {
        let mut stats = ImplicitQueryStats::default();
        stats.record("same".into(), "summary".into());

        assert_eq!(stats.total, 0);
        assert!(stats.by_fingerprint.is_empty());
    }
}
