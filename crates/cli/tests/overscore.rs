//! E2E tests for the `agent/overscore` slice: three defects around what
//! overload resolution and `new` are willing to accept, all three left reduced
//! by `agent/triemapjava` and all three making the compiler answer where it
//! should not.
//!
//! **1. Explicit type arguments did not reach the overload pick.** SLS 6.26.3
//! and nsc's `Infer.inferPolyAlternatives`: a written `[T1, …, Tn]` *is* the
//! instantiation, applied to every alternative before applicability is
//! weighed. Here the written arguments arrived only afterwards
//! (`check_apply`'s `pending_targs`), so each alternative was instantiated by
//! inferring from the value arguments alone. gitbucket's
//! `EditorConfigUtil.scala:129` writes
//!
//! ```scala
//! props.getValue[Integer](PropertyType.tab_width, TabSizeDefault, false)
//! ```
//!
//! against ec4j's two public overloads `(PropertyType[T], T, boolean)T` and
//! `(String, T, boolean)T`. Inference solved `T` to the least upper bound of
//! `Integer` (from the first argument) and `scala.Int` (from the second);
//! `PropertyType` is invariant, so the first alternative was rejected, the
//! second wants a `String`, and the call was `no matching overload`. With the
//! written `Integer` applied first, the first alternative is applicable
//! through the same `Predef.int2Integer` view that a *lone* candidate has
//! always been allowed -- which is why the call compiled with either half of
//! the pair on its own and failed only with both.
//!
//! This is the one *stated, not netted* regression of the session:
//! `agent/triemapjava` took gitbucket 270 -> 271 by reading a Java field's
//! `Signature` correctly, which is what made `tab_width` a
//! `PropertyType[Integer]` in the first place. Measured back to **270 / 79**,
//! and the error log diff is one line, at that site.
//!
//! **2. Constructor type-argument bounds were never checked.**
//! `new Bounded2[Int, String]` for `class Bounded2[K <: AnyRef, V]` compiled.
//! nsc reports it in refchecks (`checkBounds`), which is why the fixture holds
//! that violation and nothing else: with a typer error in the file, real
//! scalac never reaches refchecks and there is nothing to compare against.
//! `check_class_tparam_bounds` is the class-side twin of the existing
//! `check_tparam_bounds`, and it skips a *higher-kinded* parameter, whose
//! bound is written in that parameter's own arguments (`MapCC[X, Y] <:
//! Map[X, Y]`) and is not a proper type to compare an argument against --
//! getting that wrong cost one new `scala/collection/Iterable.scala` error the
//! first time it was measured.
//!
//! **3. An under-applied type-argument list was accepted.** `new Cell[K]` for
//! a two-parameter `Cell` drew nothing; `apply_types` reported only the
//! *over*-applied direction, so the missing arguments were quietly filled with
//! the class's own parameters.
//!
//! Measured on this tree: gitbucket **271 / 80 -> 270 / 79** (one error, at
//! `EditorConfigUtil.scala:129`, confirmed by diffing the logs and not by the
//! count); the scala library **740 / 138** with a byte-identical error log;
//! cats **182 / 71**; slick `errors=0 files_with_errors=0 classes=1490` with
//! all 1490 class files byte-identical to the pre-fix binary's; the full
//! scala/scala corpus `losses=0` with *no* row changed at all.
//!
//! `ovsc_legal.scala` is the over-reach guard for (2) and (3): every `new` in
//! it is legal Scala sitting next to something the new checks refuse, and the
//! **pre-fix binary compiles it to byte-identical class files**.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with the
//! other slices running in parallel; see `.agent-brief.md`.

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
        "scala-rs-overscore-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn compile_errors(name: &str, extra: &[&str]) -> String {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-bad"));
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} (extra={extra:?}) to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    err
}

fn run_main(out: &Path, main: &str, cp_extra: &[&str]) -> String {
    let mut cp = out.display().to_string();
    for e in cp_extra {
        cp.push(':');
        cp.push_str(e);
    }
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// (1) Explicit type arguments instantiate an overloaded callee.
//
// Which alternative runs is a run-time difference, so the fixture prints its
// own name. Real scalac 2.13.16 compiling the same source prints the three
// lines in `tests/fixtures/expected/ovsc_targs.txt`.
// ---------------------------------------------------------------------------

#[test]
fn fixtures_ovsc_targs() {
    let out = compile_fixture_with("ovsc_targs", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_main(&out, "ovsc.Main", &[]),
            expected_stdout("ovsc_targs")
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_ovsc_targs_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ovsc_targs", &["--scala-library", jar_s]);
    assert_eq!(
        run_main(&out, "ovsc.Main", &[jar_s]),
        expected_stdout("ovsc_targs")
    );
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------------------------------------
// (2) and (3): the two rejections. Both are the whole yield of their defect,
// so the message and the line are pinned. scalac 2.13.16 reports the bounds
// violation twice on line 20 (once for the `val`'s inferred type, once for the
// `new`) and the arity one once on line 16; both texts are quoted below.
// ---------------------------------------------------------------------------

#[test]
fn ovsc_ctor_type_argument_bounds_are_checked() {
    for extra in [
        vec!["--no-scala-library"],
        match scala_library_jar() {
            Some(_) => vec![
                "--scala-library",
                "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
            ],
            None => vec!["--no-scala-library"],
        },
    ] {
        let err = compile_errors("ovsc_bounds_bad", &extra);
        assert!(
            err.contains(
                "type arguments [Int,String] do not conform to class Bounded2's \
                 type parameter bounds [K <: AnyRef,V]"
            ),
            "extra={extra:?}: {err}"
        );
        // scalac reports it on the line of the `new`.
        assert!(
            err.contains("ovsc_bounds_bad.scala:20"),
            "extra={extra:?}: {err}"
        );
    }
}

#[test]
fn ovsc_under_applied_type_arguments_are_rejected() {
    for extra in [
        vec!["--no-scala-library"],
        match scala_library_jar() {
            Some(_) => vec![
                "--scala-library",
                "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
            ],
            None => vec!["--no-scala-library"],
        },
    ] {
        let err = compile_errors("ovsc_targcount_bad", &extra);
        // scalac: "wrong number of type arguments for ovsc.Cell, should be 2".
        assert!(
            err.contains("wrong number of type arguments for Cell, should be 2"),
            "extra={extra:?}: {err}"
        );
        assert!(
            err.contains("ovsc_targcount_bad.scala:16"),
            "extra={extra:?}: {err}"
        );
    }
}

// ---------------------------------------------------------------------------
// The over-reach guard. Every `new` here is legal and sits next to something
// the two new checks refuse; the pre-fix binary compiles this file to
// byte-identical class files.
// ---------------------------------------------------------------------------

#[test]
fn fixtures_ovsc_legal() {
    let out = compile_fixture_with("ovsc_legal", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_main(&out, "ovsc.Legal", &[]),
            expected_stdout("ovsc_legal")
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_ovsc_legal_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ovsc_legal", &["--scala-library", jar_s]);
    assert_eq!(
        run_main(&out, "ovsc.Legal", &[jar_s]),
        expected_stdout("ovsc_legal")
    );
    let _ = fs::remove_dir_all(&out);
}
