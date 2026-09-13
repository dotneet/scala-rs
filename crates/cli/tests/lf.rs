//! Taking scala/scala's own `src/library` from 4 typer errors to 0, and the
//! first four roots behind it in the backend (`agent/libfinal`).
//!
//! Fixture prefix: `lf_`. Every positive fixture is **run** under
//! `-Xverify:all` and its output is produced again by real scalac 2.13.16 in the
//! same suite; every negative one is refused by both compilers.
//!
//! ## Typer
//!
//!  1. **`super.m` is one member set over `this`'s base type sequence.** nsc's
//!     `typedSuper` types a bare `super` as `SuperType(clazz.thisType,
//!     intersectionType(clazz.info.parents))`, so `findMember` gathers a single
//!     set over that intersection -- `clazz`'s linearization without `clazz` --
//!     and reduces it by overriding. This compiler took the *last written*
//!     parent clause that had any concrete member of the name, which is a
//!     different class whenever the linearization disagrees with the written
//!     order: `scala.collection.mutable.ArrayDeque` mixes in
//!     `IterableFactoryDefaults` after `IndexedSeqOps`, so
//!     `super.stepper(shape)` came back at `IterableOnce`'s weaker `S` instead
//!     of `IndexedSeqOps`' `S with EfficientSplit`
//!     (`collection/mutable/ArrayDeque.scala:68`).
//!
//!     Ranking the clauses alone is not the fix and was measured costing
//!     gitbucket two errors: returning one clause's set drops the *other*
//!     clause's overloads. The reduction is `drop_overridden_at` **at `this`**,
//!     which is the only receiver that can order two members reached through
//!     different clauses.
//!
//!  2. **Alpha-conversion in that reduction.** Two traits each writing
//!     `def f[B](x: B)` have two distinct `B` symbols, so comparing the
//!     signatures literally called every polymorphic pair *different* and the
//!     sibling-override rule never fired on one; `super.f` was then `ambiguous
//!     overload` (`immutable/Range.scala:153`, `immutable/Vector.scala:196`).
//!     nsc's `matchesType` alpha-converts (`matchesQuantified`), and so does
//!     `Check::same_member_at` now -- positionally, so a different *number* of
//!     type parameters still makes the pair unequal outright.
//!
//!  3. **As-seen-from must not substitute into the arguments it just
//!     inserted.** The parent walk substituted at every class it reached, in
//!     order, so an argument a derived class put in was handed to the
//!     ancestors' substitutions as if the member's own signature had spelled
//!     it. `IterableOps[+A, +CC[_], +C].groupBy` writes
//!     `mutable.Map.empty[K, Builder[A, C]]` and reads `m.iterator`, whose
//!     `MapOps[K, V, …]` declaration is `Iterator[(K, V)]`; `V := Builder[A, C]`
//!     is right, and then `IterableOps` -- a base class of `Map`, where `A` is
//!     `(K, V)` and `C` is `Map[K, V]` -- rewrote the `A` and `C` *inside that
//!     argument*. `v.result()` became a `Map` and
//!     `result.updated(k, v.result())` a `HashMap[K, AnyRef]`
//!     (`collection/Iterable.scala:570`). The same root, not a second one, gave
//!     `PermutationsItr.init`'s `unzip` the element type `((A, Int), Int)`
//!     (`collection/Seq.scala:598,702`) -- the brief listed them as two.
//!     nsc cannot capture here because `AsSeenFromMap` is one `TypeMap` that
//!     returns `baseargs(i)` without mapping it; the walk now gathers every
//!     (class, arguments) pair and applies one substitution.
//!
//! ## Backend (reached for the first time, because the library now type-checks)
//!
//!  4. **The generic-array detour is gated on having `ScalaRunTime`, not on
//!     linking the jar.** `a(i)` / `a(i) = v` / `a.clone()` / `a.length` at an
//!     abstract element type are `scala.runtime.ScalaRunTime` calls, and the
//!     standard library supplies that object out of its own sources -- 230 of
//!     its sites are this shape. `length` was worse than refused: it emitted
//!     `arraylength` on an `Object`, which is a `VerifyError`.
//!
//!  5. **`clone`'s terminal super member is declared on `AnyRef`.**
//!     `linearize` omits Any/AnyRef/Object, and the rule that resolves such a
//!     member through the nearest concrete superclass only accepted `Any`'s
//!     `equals`/`hashCode`/`toString`. `scala.collection.mutable.Cloneable` is
//!     `override def clone(): C = super.clone().asInstanceOf[C]`, so 57
//!     standard-library classes owed an accessor that could not be emitted.
//!
//!  6. **A chained package clause whose first segment already exists.**
//!     `jvm_for_current` carried `ow.jvm_name != "scala/runtime"` from the first
//!     backend commit, so a class declared in `package scala` / `package
//!     runtime` lost its package: 143 library classes, `ScalaRunTime$` among
//!     them, were written as bare top-level names.
//!
//!  7. **A `super` call whose parent result is the parent's own type parameter**
//!     returns `Object` on the JVM while the override declares a class, and the
//!     cast nsc's erasure inserts was skipped for exactly that case. With a
//!     `match` above it the other branch supplies the narrow type, so the
//!     verifier rejects the *merge*: `scala.collection.immutable.List
//!     .appendedAll`'s `case _ => super.appendedAll(suffix)` is the shape, and
//!     it is the first method the emitted standard library loads.
//!
//! What is **not** closed: a capturing local `object`. `docs/scala-library.md`
//! has the state of the library measure and what running the emitted library
//! found beyond this.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-lf-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn compile(name: &str, out: &Path, jar: &Path, scalac_path: Option<&Path>) -> std::process::Output {
    let src = fixtures_dir().join(format!("{name}.scala"));
    match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    }
}

fn run_java(out: &Path, cp_extra: Option<&Path>, main: &str) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java {main} failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `name` (with scala-rs when `scalac_path` is `None`), run `main`, and
/// compare with `tests/fixtures/expected/<name>.txt`.
fn check_runs_main(name: &str, main: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, &out, &jar, scalac_path);
    assert!(
        output.status.success(),
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, Some(&jar), main), expected, "{name}");
    let _ = fs::remove_dir_all(&dir);
}

fn check_runs(name: &str, scalac_path: Option<&Path>) {
    check_runs_main(name, "Main", scalac_path);
}

/// `name` must be refused, and scala-rs's diagnostics must mention each
/// `needles` fragment. `scalac_path` checks that real scalac refuses it too.
fn check_rejects(name: &str, needles: &[&str], scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, &out, &jar, scalac_path);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "{name} was accepted; it must be refused:\n{text}"
    );
    // The wording differs between the two compilers, so only scala-rs's own
    // messages are matched.
    if scalac_path.is_none() {
        for n in needles {
            assert!(text.contains(n), "{name}: expected {n:?} in:\n{text}");
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Roots 1, 2, 5 and 7: `super`.

#[test]
fn super_roots_run() {
    check_runs("lf_super", None);
}

#[test]
fn scalac_agrees_super_roots() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lf_super", Some(&sc));
}

#[test]
fn super_roots_still_reject() {
    check_rejects(
        "lf_super_bad",
        &[
            // Gathering the whole base type sequence still finds nothing when
            // no parent has the name.
            "value m is not a member of",
            // `super[P]` still demands that `P` be a parent clause.
            "`super` has no parent type",
            // Two genuine overloads in unrelated mixins are still two: the
            // message names both alternatives. Matched in pieces, because which
            // one is named first is the base-type-sequence order of their owners
            // and not something this fixture is about.
            "no matching overload for <overload ",
            "(String)Int",
            "(Int)Int",
            "with arguments (true)",
        ],
        None,
    );
}

#[test]
fn scalac_agrees_super_roots_reject() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lf_super_bad", &[], Some(&sc));
}

// ---------------------------------------------------------------------------
// Root 3: as-seen-from capture.

#[test]
fn as_seen_from_roots_run() {
    check_runs("lf_seen", None);
}

#[test]
fn scalac_agrees_as_seen_from_roots() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lf_seen", Some(&sc));
}

#[test]
fn as_seen_from_roots_still_reject() {
    check_rejects(
        "lf_seen_bad",
        &[
            // The member's own result is still substituted, once.
            "found: Elem[A, C]  required: C",
            // And the parameter is still read at the base class's arguments.
            "(Cell[Elem[A, C]])Elem[A, C] with arguments (Cell[A])",
            "(Cell[Elem[Int, String]])Elem[Int, String] with arguments (Cell[Int])",
        ],
        None,
    );
}

#[test]
fn scalac_agrees_as_seen_from_roots_reject() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lf_seen_bad", &[], Some(&sc));
}

// ---------------------------------------------------------------------------
// Root 6: the binary name of a chained package clause.

#[test]
fn chained_package_clause_keeps_its_package() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("pkgname");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile("lf_pkgname", &out, &jar, None);
    assert!(
        output.status.success(),
        "compile lf_pkgname failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for f in ["LfPkgCell.class", "LfPkgMain.class", "LfPkgMain$.class"] {
        assert!(
            out.join("scala/runtime").join(f).is_file(),
            "scala/runtime/{f} was not written; tree is {:?}",
            fs::read_dir(&out).map(|d| d.flatten().map(|e| e.path()).collect::<Vec<_>>())
        );
        assert!(
            !out.join(f).is_file(),
            "{f} was written in the default package"
        );
    }
    assert_eq!(
        run_java(&out, Some(&jar), "scala.runtime.LfPkgMain"),
        fs::read_to_string(fixtures_dir().join("expected/lf_pkgname.txt")).unwrap()
    );
    let _ = fs::remove_dir_all(&dir);
}

/// ...and real scalac writes it in the same place.
#[test]
fn scalac_agrees_chained_package_clause() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or jar not present");
        return;
    };
    let dir = tmp_dir("pkgname-sc");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile("lf_pkgname", &out, &jar, Some(&sc));
    assert!(output.status.success());
    assert!(out.join("scala/runtime/LfPkgCell.class").is_file());
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Root 4: the generic-array detour, gated on the run having `ScalaRunTime`.
//
// `--no-scala-library` only: real scalac always links the jar's own
// `ScalaRunTime` and would see a duplicate definition.

#[test]
fn generic_array_access_uses_the_runs_own_scala_run_time() {
    let expected = fs::read_to_string(fixtures_dir().join("expected/lf_srt.txt")).unwrap();
    let dir = tmp_dir("srt");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(fixtures_dir().join("lf_srt.scala"))
        .args(["-d", out.to_str().unwrap()])
        .arg("--no-scala-library")
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "lf_srt did not compile:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, None, "Main"), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// The other direction, and the reason the gate is a gate: the same shape with
/// no `ScalaRunTime` anywhere in the run is still a diagnostic.
/// `tests/fixtures/arraygen_gate.scala` is that file (`arraygen.rs` pins it);
/// this asserts it here too, so removing the gate cannot pass unnoticed.
#[test]
fn generic_array_access_without_scala_run_time_is_still_refused() {
    let dir = tmp_dir("srt-gate");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(fixtures_dir().join("arraygen_gate.scala"))
        .args(["-d", out.to_str().unwrap()])
        .arg("--no-scala-library")
        .output()
        .expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "accepted:\n{text}");
    assert!(text.contains("ClassTag"), "{text}");
    let _ = fs::remove_dir_all(&dir);
}
