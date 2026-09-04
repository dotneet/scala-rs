//! E2E tests for the `agent/vcself` slice (fixture prefix `vcself`).
//!
//! Three defects. Together they took `tests/slick_run.sh` from `ok=4 diff=2
//! fail=6` to `ok=10 diff=1 fail=1`.
//!
//! 1. **Inside a value class, a call to another of its own methods did not
//!    reach the underlying value.** Every method of `class C(val u: U) extends
//!    AnyVal` is really a static taking `u`, and `this` is the box. The
//!    instance method passed `this` straight into `b$extension(U, …)`, and the
//!    `$extension` static -- whose slot 0 already holds `u` -- re-boxed it with
//!    a `new C(u)` first. nsc emits `aload_0; invokevirtual u()` in the one and
//!    the bare slot in the other. The second shape only fails at run time when
//!    `U` is an interface (JVMS 4.10.1.2 makes every reference assignable to
//!    one), which is why neither `slick_subset.sh` nor `javap` saw it: slick's
//!    `AnyOptionExtensionMethods.map$extension` handed `OptionLift.baseValue` a
//!    wrapper (`scala.MatchError`) and `ActionBasedSQLInterpolation.sqlu` was a
//!    `VerifyError`.
//!
//! 2. **An erasure bridge whose implementation returns `Unit` returned
//!    nothing.** `Unit` is `V` in the implementation's own descriptor while
//!    the bridge owes a reference; nsc pushes `BoxedUnit.UNIT`. `param_adapt`'s
//!    `Unit` rule is the *parameter* one -- a `Unit` argument really does
//!    arrive as a `BoxedUnit` reference -- and said nothing about the result.
//!    slick's `implicit object SetUnit extends SetParameter[Unit]`, over
//!    `SetParameter[-T] extends ((T, PositionedParameters) => Unit)`, was
//!    `VerifyError: Operand stack underflow`.
//!
//! 3. **A `val` narrowed by an override in a subclass got no wide getter.**
//!    `emit_inherited_covariant_bridges` took a `val` only from a *trait*
//!    parent, so a class parent's own methods read the base's field. slick's
//!    `QueryBuilder.quotedJdbcFns: Option[Seq[JdbcFunction]] = None`, narrowed
//!    to `Some[Nil.type]` by `H2Profile`'s subclass, kept quoting every JDBC
//!    function: `{fn length("NAME")}` where H2 wants `length("NAME")`.
//!
//! Self-contained (own copies of the small helpers `e2e.rs` also has) per
//! `.agent-brief.md`: the shared test files belong to other in-flight agents.

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
        "scala-rs-vcself-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Compile the fixture against the real scala-library; returns the output dir.
fn compile_fixture() -> Option<PathBuf> {
    let jar = scala_library_jar()?;
    let out = tmp_dir("out");
    let res = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("vcself.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        res.status.success(),
        "scala-rs failed on vcself.scala:\n{}\n{}",
        String::from_utf8_lossy(&res.stdout),
        String::from_utf8_lossy(&res.stderr)
    );
    Some(out)
}

fn javap(out: &Path, class: &str) -> Option<String> {
    let res = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .ok()?;
    res.status
        .success()
        .then(|| String::from_utf8_lossy(&res.stdout).into_owned())
}

/// The body of the first method whose signature line contains `sig`.
fn method_body(text: &str, sig: &str) -> String {
    let start = text
        .find(sig)
        .unwrap_or_else(|| panic!("no method matching `{sig}` in:\n{text}"));
    let rest = &text[start..];
    let end = rest[1..].find("\n\n").map(|i| i + 1).unwrap_or(rest.len());
    rest[..end].to_string()
}

/// The whole fixture under the bytecode verifier, against real scalac
/// 2.13.16's own stdout for the same file.
#[test]
fn fixtures_vcself_matches_scalac() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip vcself: no scala-library jar");
        return;
    };
    let out = compile_fixture().unwrap();
    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "Main",
        ])
        .output()
        .expect("run the fixture");
    assert!(
        run.status.success(),
        "vcself failed at run time:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let expected = fs::read_to_string(fixtures_dir().join("expected").join("vcself.txt")).unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = fs::remove_dir_all(&out);
}

/// The two receiver shapes, which stdout cannot tell apart from a wrapper that
/// happens to survive the call: the instance method unwraps `this` through the
/// accessor, and the `$extension` static passes its own slot 0 on untouched.
#[test]
fn fixtures_vcself_self_call_passes_the_underlying_value() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip vcself: no scala-library jar");
        return;
    };
    let ops = javap(&out, "VcOps").expect("javap VcOps");
    for sig in [
        "public java.lang.String bang(int)",
        "public java.lang.String query(int)",
    ] {
        let body = method_body(&ops, sig);
        assert!(
            body.contains("invokevirtual") && body.contains("Method s:()Ljava/lang/String;"),
            "{sig} does not unwrap `this`:\n{body}"
        );
        assert!(
            !body.contains("class VcOps"),
            "{sig} still passes the box:\n{body}"
        );
    }
    for sig in [
        "public static java.lang.String bang$extension(java.lang.String, int)",
        "public static java.lang.String query$extension(java.lang.String, int)",
    ] {
        let body = method_body(&ops, sig);
        assert!(
            !body.contains("class VcOps"),
            "{sig} re-boxes its own underlying value:\n{body}"
        );
        assert!(
            body.contains("rep$extension"),
            "{sig} does not reach rep$extension:\n{body}"
        );
    }
    // A primitive underlying: the same two shapes, with the accessor typed `D`.
    let cel = javap(&out, "VcCel").expect("javap VcCel");
    let warmer = method_body(&cel, "public double warmer(double)");
    assert!(
        warmer.contains("Method c:()D") && !warmer.contains("class VcCel"),
        "VcCel.warmer does not unwrap `this`:\n{warmer}"
    );
    let warmer_ext = method_body(
        &cel,
        "public static double warmer$extension(double, double)",
    );
    assert!(
        !warmer_ext.contains("class VcCel"),
        "VcCel.warmer$extension re-boxes its own underlying value:\n{warmer_ext}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// An erasure bridge over a `Unit`-returning implementation has to materialise
/// `BoxedUnit.UNIT`; without it the `areturn` pops an empty stack.
#[test]
fn fixtures_vcself_unit_bridge_returns_the_singleton() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip vcself: no scala-library jar");
        return;
    };
    for class in ["VcSinkUnit$", "VcSinkInt$"] {
        let text = javap(&out, class).expect("javap sink");
        let body = method_body(
            &text,
            "public java.lang.Object apply(java.lang.Object, java.lang.Object)",
        );
        assert!(
            body.contains("BoxedUnit.UNIT"),
            "{class}'s erased apply does not push the Unit singleton:\n{body}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// A `val` narrowed in a subclass owes the base class's descriptor as a
/// bridge. The stdout above sees it only because `VcBuilder.show` reads the
/// member; the descriptor is what a separately compiled caller links against.
#[test]
fn fixtures_vcself_narrowed_val_gets_the_wide_getter() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip vcself: no scala-library jar");
        return;
    };
    let text = javap(&out, "VcH2Builder").expect("javap VcH2Builder");
    for (sig, target) in [
        (
            "public scala.Option quoted()",
            "Method quoted:()Lscala/Some;",
        ),
        (
            "public scala.collection.immutable.Seq plain()",
            "Method plain:()Lscala/collection/immutable/List;",
        ),
    ] {
        let body = method_body(&text, sig);
        assert!(
            body.contains("invokevirtual") && body.contains(target),
            "{sig} is not a bridge to the override:\n{body}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
