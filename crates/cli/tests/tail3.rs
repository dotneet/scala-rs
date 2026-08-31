//! E2E tests for the `agent/tail3` slice: three slick `error:` clusters that
//! turned out to share one shape -- something binding, resolving, or being
//! rewritten at the *wrong* type or in the *wrong* pieces -- but had
//! different root causes and different fixes. Fixture prefix `t3`. Kept out
//! of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`.
//!
//! # (1) `value volatileHint is not a member of Node` (3 occurrences)
//!
//! `slick/jdbc/{DerbyProfile,JdbcStatementBuilderComponent,SQLServerProfile}
//! .scala` all match `case c @ LiteralNode(_) if c.volatileHint => …` (or the
//! `:@` infix form) against a `Node`-typed scrutinee, where `volatileHint` is
//! declared on `LiteralNode`, not `Node`. `LiteralNode` is not a `case
//! class`; it has a hand-written companion `def unapply(n: LiteralNode):
//! Option[Any]`. Real scalac types `c` (an `x @ Extractor(...)` binding) at
//! the extractor's own declared receiver type -- exactly the implicit type
//! test a bare `case x: T` performs -- but `crates/typer/src/check.rs`'s
//! `unapply` arm of `type_pattern` always set the whole pattern's type
//! (`pat.ty`) to the *scrutinee's* type, so `c` stayed a `Node` and
//! `c.volatileHint` was rejected.
//!
//! The fix narrows in exactly one place: `TreeKind::Bind`, after typing the
//! inner pattern. The `TreeKind::UnApply` node's own `pat.ty` is deliberately
//! left as the scrutinee's type -- `crates/backend/src/gen.rs`'s
//! `gen_unapply_pattern` reads it back to decide whether the extractor's
//! runtime `instanceof` test is redundant (`is_sub_type(pat.ty, param_ty)`),
//! and narrowing it there made that check trivially true, which skipped the
//! test entirely: `describe(new OtherNode)` (matching neither `LiteralNode`
//! case) reached the first case's `Bind` codegen anyway and threw
//! `ClassCastException: OtherNode cannot be cast to LiteralNode` instead of
//! falling through to the next case. Caught by running `t3_extractor_bind`
//! under `-Xverify:all` *and* comparing stdout against real scalac before
//! trusting the fix -- the typer-only version compiled clean and only broke
//! at run time.
//!
//! `unapply_receiver_type` (`crates/typer/src/check.rs`) computes the
//! narrowed type: the extractor's declared parameter type, with its own type
//! parameters unified against the scrutinee (mirrors `subst_unapply_tparams`,
//! which already did this for the sub-pattern types) -- covering a generic
//! extractor (`NonEmptyBox.unapply[T](b: Box[T]): Option[T]`) as well as a
//! monomorphic one. `t3_extractor_bind.scala` exercises both.
//!
//! # (2) `recursive method computeCapabilities needs result type` (3 occurrences)
//!
//! `slick/{jdbc/DB2Profile,relational/RelationalProfile,sql/SqlProfile}.scala`
//! each override `computeCapabilities` (no declared result type) as
//! `super.computeCapabilities ++ …Capabilities.all`. The base case
//! (`BasicProfile.computeCapabilities: Set[Capability] = Set.empty`) has an
//! explicit type, so nothing here should be a real cycle -- confirmed with
//! real scalac (`t3_super_chain.scala` compiles clean) before touching
//! anything, per the brief's "recursive needs result type" note. (The
//! DB2Profile.scala:30 occurrence is a fourth, unrelated span bug: it points
//! into a doc comment, not at `computeCapabilities` at all, and was not
//! investigated further here.)
//!
//! Two independent bugs stacked to cause this:
//!
//! * **Typer**: `RelationalProfile extends BasicProfile with
//!   RelationalTableComponent with … with RelationalActionComponent`, and
//!   `RelationalActionComponent { self: RelationalProfile => }` is one of the
//!   *later*-listed (and so, in `super_target`'s old "last parent" heuristic,
//!   `super`-preferred) parents. `SymbolTable::lookup_member` -- the general
//!   member search used for an ordinary selection -- also walks a class's
//!   `self_type`, which is right for `this.foo` / an unqualified reference
//!   *inside* a self-typed trait's own body, but SLS 6.7.3 never lets `super`
//!   reach a member through a self-type: only real `extends`/`with` parents.
//!   `super.computeCapabilities` inside `RelationalProfile` resolved (through
//!   `RelationalActionComponent`'s self-type) back to `RelationalProfile`'s
//!   *own*, still-being-completed override -- a genuine cyclic reference, just
//!   not the one nsc reports, because nsc never takes that path. Fixed with
//!   `SymbolTable::lookup_member_real` (walks real parents only) and
//!   `Typer::super_select_member` (searches `this_id`'s real parents,
//!   last-declared first, matching Scala's linearization preference, until
//!   one's real inheritance chain defines the wanted name), wired into
//!   `type_select` specifically when the qualifier is a `Super` tree.
//!
//! * **Backend**: fixing the typer bug above got the fixture past
//!   typechecking, but `ClassImpl` (a plain `class`) and `ObjectImpl` (an
//!   `object`) mixing in the same trait chain gave different answers --
//!   `ObjectImpl.m` threw `AbstractMethodError: … Mid$$super$m() of interface
//!   Mid`. `crates/backend/src/gen.rs`'s `emit_class` calls
//!   `emit_super_accessors` (which implements every mixed-in trait's abstract
//!   `Trait$$super$m` accessor -- the mechanism a trait's own `super.m` call
//!   compiles to, since JVM interfaces cannot resolve `super` themselves) but
//!   `emit_module` (an `object`'s own, separate codegen path) never did. Every
//!   `object Foo extends SomeTrait` where `SomeTrait` (or something it mixes
//!   in) calls `super` from its own body -- exactly slick's per-database
//!   profile objects (`object H2Profile extends JdbcProfile`, etc.) -- hit
//!   this, just never through a compiling program before (1) above always
//!   rejected the pattern first). One line added:
//!   `self.emit_super_accessors(&mut b, cls);` in `emit_module`.
//!
//! `t3_super_chain.scala` pins both: `Base` (a real superclass, so no
//! accessor needed there), `CompA` / `CompB` (self-typed, no `m` of their
//! own -- the typer bug's shape), `Mid` and `Top` each `override def m =
//! super.m + …` (no declared result type -- the diagnostic's shape), mixed
//! into both a `class` and an `object` (the backend bug's shape). Dual-run
//! under both `--scala-library` and `--no-scala-library`, `-Xverify:all`,
//! and directly against real scalac's stdout.
//!
//! # (3) `value apply is not a member of TableNode` (3 occurrences, +2 cascades)
//!
//! `slick/ast/Node.scala` declares `final case class TableNode(schemaName,
//! tableName, identity, baseIdentity)(val profileTable: Any)` -- a *curried*
//! case class, its second parameter list a single `val`. Real uses (`slick/
//! compiler/{AssignUniqueSymbols,EmulateOuterJoins}.scala`) write `t.copy
//! (identity = x)(t.profileTable)`, mirroring the constructor's own two
//! lists.
//!
//! `Typer::try_rewrite_case_copy` (`crates/typer/src/check.rs`) rewrites
//! `p.copy(…)` directly to a constructor call rather than emitting a real
//! call to the synthetic `copy` method, to reuse ordinary constructor-call
//! type inference instead of re-implementing `copy[T]`'s own. It is invoked
//! per single `Apply` node, so on `t.copy(identity = x)(t.profileTable)` it
//! ran on the *inner* `Apply` alone (`t.copy(identity = x)`) before the
//! *outer* one (supplying `(t.profileTable)`) was even considered -- filled
//! every field, including ones belonging to the second list, from `t`'s own
//! value, and returned a complete `TableNode`. The outer `(t.profileTable)`
//! then read as an attempt to call `.apply` on that already-complete value:
//! "value apply is not a member of TableNode". (Confirmed genuinely curried,
//! not flattened, by checking what `r.copy(a = 2)(r.extra)` compiles to with
//! real scalac 2.13.16 before touching anything -- the *type* stays curried
//! even though, like the constructor itself, it erases to one flat JVM
//! method either way, which is why comparing bytecode alone cannot tell the
//! two shapes apart.)
//!
//! Fixed with `Typer::try_rewrite_case_copy_curried`, tried first: it peels
//! the whole `Apply` chain down to the `copy` selection, and if it is at
//! least two layers deep, rebuilds one `new`-free call chain --
//! `ClassName(list1)(list2)…`, going through the companion's own (already
//! correctly curried) `apply` rather than `new C(…)(…)`, which turned out to
//! have a *separate*, narrower gap in curried constructor-call overload
//! resolution (checked one `Apply` layer in isolation) that building on it
//! here would have traded one bug for another. Falls through to the
//! existing single-list rewrite when there is no second list to peel (depth
//! < 2), so the common, non-curried case is untouched.
//!
//! `t3_curried_copy.scala` exercises a named argument in each list, a
//! positional argument in the second list, and every field defaulted in
//! both lists (`r.copy()(r.extra)`) -- dual-run under both library modes,
//! `-Xverify:all`, and directly against real scalac's stdout.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-tail3-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    out
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `--no-scala-library` (private runtime) check.
fn check(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let got = run_java(&out, None);
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    let p = Command::new("scalac").arg("-version").output().ok()?;
    (p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty())
        .then_some(PathBuf::from("scalac"))
}

/// `--scala-library` (real jar) dual-run, under `-Xverify:all`.
fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run {name}: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    let got = run_java(&out, Some(jar_s));
    assert_eq!(
        got,
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Compiles the fixture with the *real* scalac 2.13.16 and checks its stdout
/// against the recorded expectation -- confirms the fixture is legitimate
/// Scala the diagnostics brief asks every subagent to doubt, not just a shape
/// our own compiler happens to like.
fn real_scalac_check(name: &str) {
    if !java_available() {
        return;
    }
    let Some(scalac) = scalac() else {
        eprintln!("skip real-scalac check {name}: scalac not obtainable");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip real-scalac check {name}: scala-library jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let out = run_java(&ref_out, Some(jar.to_str().unwrap()));
    assert_eq!(
        out,
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

// --- (1) `x @ Extractor(...)` narrows to the extractor's receiver type ----

#[test]
fn fixtures_t3_extractor_bind() {
    check("t3_extractor_bind");
}

#[test]
fn scala_library_dual_run_t3_extractor_bind() {
    dual_run_fixture("t3_extractor_bind");
}

#[test]
fn real_scalac_accepts_t3_extractor_bind() {
    real_scalac_check("t3_extractor_bind");
}

// --- (2) `super.m` walks real parents, never a self-type ------------------

#[test]
fn fixtures_t3_super_chain() {
    check("t3_super_chain");
}

#[test]
fn scala_library_dual_run_t3_super_chain() {
    dual_run_fixture("t3_super_chain");
}

#[test]
fn real_scalac_accepts_t3_super_chain() {
    real_scalac_check("t3_super_chain");
}

// --- (3) `p.copy(...)( ...)` peels the whole curried chain first ----------

#[test]
fn fixtures_t3_curried_copy() {
    check("t3_curried_copy");
}

#[test]
fn scala_library_dual_run_t3_curried_copy() {
    dual_run_fixture("t3_curried_copy");
}

#[test]
fn real_scalac_accepts_t3_curried_copy() {
    real_scalac_check("t3_curried_copy");
}
