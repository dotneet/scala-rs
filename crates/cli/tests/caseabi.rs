//! E2E tests for the `agent/caseabi` slice: the `hashCode` SLS 5.3.2
//! synthesizes for a `case class`.
//!
//! The backend always emitted *a* `hashCode`. Under `--scala-library` it was
//! not **nsc's**: it folded the fields with 31 where nsc mixes them with
//! `MurmurHash3` / `scala.runtime.Statics`, so `Point(1, "a").hashCode` was
//! `128` here and `-1322997830` under scalac. Nothing broke internally — the
//! 31-fold agrees with our own `equals` — but a case class we compile and one
//! scalac compiles **hashed differently**, and any `HashMap` that saw both
//! missed. `nsc_compiled_app_shares_a_hashmap_with_our_case_classes` below is
//! the check that reaches that: scala-rs compiles the case classes, real
//! scalac 2.13.16 compiles a program that puts them in a `Map`, a
//! `java.util.HashMap` and a `Set`, and the two halves run together.
//!
//! The 31-fold stays under `--no-scala-library`: `scala.runtime.Statics` and
//! `scala.runtime.ScalaRunTime$` are library classes the private runtime does
//! not have, and nothing in that mode ever meets a scalac-compiled case class,
//! so there is no hash to agree with. Its numbers therefore differ from
//! scalac's **by design**, and are pinned separately in
//! `tests/fixtures/expected/caseabi_hash_priv.txt`.
//!
//! A `hashCode` that merely compiles proves nothing, so every positive fixture
//! **executes and prints actual hash values**, diffed byte for byte against
//! real scalac 2.13.16 compiling the same source.
//!
//! Kept separate from `crates/cli/tests/e2e.rs`, `crates/cli/tests/caseeq.rs`
//! and `crates/cli/tests/product.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `caseabi` prefix.

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
        "scala-rs-caseabi-{tag}-{}-{nanos}-{seq}",
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

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    let which = Command::new("which").arg("scalac").output().ok()?;
    which
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&which.stdout).trim().to_string()))
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn compile_fixture_into(name: &str, out: &Path, extra: &[&str]) {
    let src = fixtures_dir().join(format!("{name}.scala"));
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
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let out = tmp_dir(name);
    compile_fixture_into(name, &out, extra);
    out
}

fn run_scalac(sc: &Path, args: &[&str]) {
    let output = Command::new(sc).args(args).output().expect("run scalac");
    assert!(
        output.status.success(),
        "scalac {args:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed on {cp}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    run_main(&cp)
}

fn javap(out: &Path, class: &str) -> String {
    let text = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    String::from_utf8_lossy(&text.stdout).into_owned()
}

/// The `hashCode` body of `class`, sliced out of `javap -p -c`.
fn hash_code_body(out: &Path, class: &str) -> String {
    let text = javap(out, class);
    let start = text
        .find("public int hashCode();")
        .unwrap_or_else(|| panic!("no hashCode in {class}:\n{text}"));
    let rest = &text[start..];
    let end = rest
        .find("\n\n")
        .map(|i| start + i)
        .unwrap_or_else(|| text.len());
    text[start..end].to_string()
}

/// The `true`/`false` lines of a fixture's expected output — the part that
/// says which values hash alike, and so must not depend on the mode.
fn invariant_lines(name: &str) -> Vec<String> {
    expected_stdout(name)
        .lines()
        .filter(|l| *l == "true" || *l == "false")
        .map(str::to_string)
        .collect()
}

// ------------------------------------------------------------- both modes

/// Private runtime (`--no-scala-library`): the 31-fold, whose numbers are
/// *not* scalac's and are pinned on their own. What must still hold there is
/// the invariant half of the fixture — equal values hash equal, different
/// values hash differently — which is why those `true`/`false` lines are the
/// same in both expected files.
#[test]
fn fixtures_caseabi_hash_private_runtime() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with("caseabi_hash", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        expected_stdout("caseabi_hash_priv"),
        "stdout mismatch for private-runtime caseabi_hash"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Library ABI (`--scala-library`): nsc's own numbers.
#[test]
fn fixtures_caseabi_hash_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip caseabi library run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("caseabi_hash", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("caseabi_hash"),
        "stdout mismatch for library caseabi_hash"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The `true`/`false` tail of the fixture is `hashCode`'s agreement with
/// `equals`. That part does not depend on the mode, and pinning it here says
/// so: if the private runtime's numbers ever stop separating values the
/// library mode's numbers separate, this fails even though both expected
/// files are self-consistent.
#[test]
fn caseabi_hash_agrees_with_equals_in_both_modes() {
    let lib = invariant_lines("caseabi_hash");
    let priv_ = invariant_lines("caseabi_hash_priv");
    assert!(!lib.is_empty(), "fixture lost its invariant lines");
    assert_eq!(
        lib, priv_,
        "the two modes disagree about which values hash alike"
    );
}

// -------------------------------------------------- diffed against scalac

/// The recorded expectation *is* real scalac 2.13.16's stdout, and ours has to
/// match it byte for byte. This is the check that matters: the old 31-fold
/// compiled, ran, and answered a number scalac never produces.
#[test]
fn real_scalac_dual_run_caseabi_hash() {
    real_scalac_dual_run("caseabi_hash");
}

/// The same, for the shapes that decide *which* of nsc's two `hashCode`
/// bodies gets emitted: a value class (boxed back up before `anyHash`), a type
/// parameter, `Any`, `Array`, `Unit`, and an all-reference case class.
#[test]
fn real_scalac_dual_run_caseabi_hash_lib() {
    real_scalac_dual_run("caseabi_hash_lib");
}

fn real_scalac_dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip {name} real-scalac diff: scalac or jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    run_scalac(
        &sc,
        &[src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()],
    );
    assert_eq!(
        run_java(&ref_out, Some(jar_s)),
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout(name),
        "scala-rs and real scalac disagree about {name}"
    );
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------ the shapes

/// nsc has two synthesized `hashCode` bodies and `SyntheticMethods
/// .chooseHashcode` picks between them: the inline `MurmurHash3` mix chain
/// when at least one case accessor has a primitive value type, and a forward
/// to `ScalaRunTime$._hashCode(this)` otherwise. The values agree either way,
/// so only `javap` can tell whether we reproduced the split or merely landed
/// on the same numbers.
#[test]
fn caseabi_hash_has_nscs_two_shapes() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip caseabi shape check: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("caseabi_hash", &["--scala-library", jar_s]);
    let out2 = compile_fixture_with("caseabi_hash_lib", &["--scala-library", jar_s]);

    for (dir, class) in [(&out, "Point"), (&out, "Wide"), (&out2, "Mixed")] {
        let body = hash_code_body(dir, class);
        assert!(
            body.contains("int -889275714"),
            "{class} does not seed with MurmurHash3.productSeed:\n{body}"
        );
        assert!(
            body.contains("Method productPrefix:()Ljava/lang/String;"),
            "{class} does not mix in productPrefix:\n{body}"
        );
        assert!(
            body.contains("scala/runtime/Statics.mix:(II)I")
                && body.contains("scala/runtime/Statics.finalizeHash:(II)I"),
            "{class} is not a Statics mix chain:\n{body}"
        );
        assert!(
            !body.contains("java/util/Objects.hashCode"),
            "{class} still folds with java.util.Objects (the 31-fold):\n{body}"
        );
    }

    // No primitive accessor at all -> the whole thing goes to the runtime.
    // `Zero()` has no accessors; `OnlyStr`/`Pair` have only references; `Box`'s
    // `Meters` is a *value* class, which is not primitive for this test even
    // though the field is stored as the underlying `Int`.
    for (dir, class) in [
        (&out, "Zero"),
        (&out, "OnlyStr"),
        (&out2, "Box"),
        (&out2, "Cell"),
        (&out2, "AnyF"),
        (&out2, "Pair"),
    ] {
        let body = hash_code_body(dir, class);
        assert!(
            body.contains("scala/runtime/ScalaRunTime$._hashCode:(Lscala/Product;)I"),
            "{class} should forward to ScalaRunTime._hashCode:\n{body}"
        );
        assert!(
            !body.contains("Statics.mix"),
            "{class} should not carry an inline mix chain:\n{body}"
        );
    }

    // Per-field: `Long`/`Double`/`Float` get their own `Statics` hash so that
    // `1.0.##` and `1.##` agree; a `Boolean` is `1231`/`1237` inline; a value
    // class is boxed back up before `anyHash`.
    let wide = hash_code_body(&out, "Wide");
    for needle in [
        "scala/runtime/Statics.longHash:(J)I",
        "scala/runtime/Statics.doubleHash:(D)I",
        "scala/runtime/Statics.anyHash:(Ljava/lang/Object;)I",
        "sipush        1231",
        "sipush        1237",
    ] {
        assert!(wide.contains(needle), "Wide is missing {needle}:\n{wide}");
    }
    let floaty = hash_code_body(&out, "Floaty");
    assert!(
        floaty.contains("scala/runtime/Statics.floatHash:(F)I"),
        "Floaty is missing floatHash:\n{floaty}"
    );
    let mixed = hash_code_body(&out2, "Mixed");
    assert!(
        mixed.contains("class Meters")
            && mixed.contains("Method Meters.\"<init>\":(I)V")
            && mixed.contains("scala/runtime/Statics.anyHash"),
        "Mixed must box its value-class field before anyHash:\n{mixed}"
    );

    // A hand-written `hashCode` wins, in both shapes' territory.
    let custom = hash_code_body(&out, "Custom");
    assert!(
        custom.contains("sipush        4242") && !custom.contains("Statics.mix"),
        "a hand-written hashCode was replaced by the synthesized one:\n{custom}"
    );

    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&out2);
}

/// The private runtime keeps the 31-fold, and has to: neither
/// `scala.runtime.Statics` nor `scala.runtime.ScalaRunTime$` exists there, so
/// emitting nsc's body would produce class files that do not link.
#[test]
fn caseabi_private_runtime_keeps_the_31_fold() {
    let out = compile_fixture_with("caseabi_hash", &["--no-scala-library"]);
    let point = hash_code_body(&out, "Point");
    assert!(
        point.contains("java/util/Objects.hashCode:(Ljava/lang/Object;)I")
            && point.contains("bipush        31"),
        "the private runtime's hashCode is no longer the 31-fold:\n{point}"
    );
    assert!(
        !point.contains("scala/runtime/Statics") && !point.contains("ScalaRunTime"),
        "the private runtime's hashCode names a library class it does not have:\n{point}"
    );
    // Including the shape that goes to the runtime under `--scala-library`.
    let zero = hash_code_body(&out, "Zero");
    assert!(
        !zero.contains("ScalaRunTime"),
        "zero-field case class reaches for ScalaRunTime without the jar:\n{zero}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------- the point

/// The one that settles it. scala-rs compiles the case classes; **real scalac
/// 2.13.16** compiles a program that puts them in a `scala.collection.Map`, a
/// `java.util.HashMap` and a `Set`, building some keys on each side; the two
/// run together and must print exactly what scalac-on-scalac prints.
///
/// Before this slice the two halves disagreed: `Key(1, "a")` hashed to the
/// 31-fold on our side and to `1584692233` on scalac's, so every cross-half
/// lookup missed and every `Set` of two equal values had two elements.
#[test]
fn nsc_compiled_app_shares_a_hashmap_with_our_case_classes() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip caseabi hashing interop: needs the scala-library jar and scalac 2.13.16");
        return;
    };
    let app = fixtures_dir().join("caseabi_app.scala");

    // Control: scalac compiles both halves.
    let n_lib = tmp_dir("nsc-lib");
    let n_app = tmp_dir("nsc-app");
    run_scalac(
        &sc,
        &[
            "-d",
            n_lib.to_str().unwrap(),
            fixtures_dir().join("caseabi_lib.scala").to_str().unwrap(),
        ],
    );
    run_scalac(
        &sc,
        &[
            "-cp",
            n_lib.to_str().unwrap(),
            "-d",
            n_app.to_str().unwrap(),
            app.to_str().unwrap(),
        ],
    );
    let control = run_main(&format!(
        "{}:{}:{}",
        n_lib.display(),
        n_app.display(),
        jar.display()
    ));
    assert!(
        control.contains("Some(one)") && control.trim_end().ends_with('1'),
        "the scalac-on-scalac control did not do what the fixture describes:\n{control}"
    );

    // The real thing: scala-rs compiles the case classes, scalac the program.
    let r_lib = tmp_dir("rs-lib");
    let r_app = tmp_dir("nsc-over-rs");
    compile_fixture_into(
        "caseabi_lib",
        &r_lib,
        &["--scala-library", jar.to_str().unwrap()],
    );
    run_scalac(
        &sc,
        &[
            "-cp",
            r_lib.to_str().unwrap(),
            "-d",
            r_app.to_str().unwrap(),
            app.to_str().unwrap(),
        ],
    );
    let ours = run_main(&format!(
        "{}:{}:{}",
        r_lib.display(),
        r_app.display(),
        jar.display()
    ));

    assert_eq!(
        ours, control,
        "case classes scala-rs compiled do not hash like scalac's inside a \
         HashMap real scalac compiled"
    );
    for d in [n_lib, n_app, r_lib, r_app] {
        let _ = fs::remove_dir_all(d);
    }
}

// ------------------------------------------------------------- negative

/// The synthesized `hashCode` steps aside for a hand-written one, but a
/// hand-written one that does not override `Object.hashCode` is still an
/// error — in both modes, as it is under scalac.
#[test]
fn caseabi_hash_bad_is_rejected_in_both_modes() {
    let Some(jar) = scala_library_jar() else {
        compile_fails("caseabi_hash_bad", &["--no-scala-library"]);
        return;
    };
    compile_fails("caseabi_hash_bad", &["--no-scala-library"]);
    compile_fails(
        "caseabi_hash_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
}

fn compile_fails(name: &str, extra: &[&str]) {
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
    assert!(
        err.contains("incompatible type in overriding"),
        "expected {name} to be rejected as a bad override, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
