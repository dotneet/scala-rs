//! E2E tests for the `agent/selfrec` slice (fixture prefix `selfrec`).
//!
//! Two defects, and between them they took `tests/slick_run.sh` from
//! `ok=0 diff=0 fail=12` to its first completed programs.
//!
//! 1. **The receiver hoisted for an omitted default wrapped the wrong node.**
//!    `default_recv::hoist_default_receivers` binds a computed receiver to a
//!    local so it runs once, and it ran bottom-up on each `Apply`. For a
//!    curried call whose default sits in a clause that is not the last one it
//!    therefore wrapped the *inner* application, leaving
//!    `Apply { fun: Block { … }, args }` -- a callee with no symbol, which
//!    `gen_apply` emits as `throw new RuntimeException("unresolved apply")`.
//!    slick's
//!
//!    ```scala
//!    createInvoker(statements).foreach(x => b += x)(ctx.session)
//!    // final def foreach(f: R => Unit, maxRows: Int = 0)(implicit s: Session)
//!    ```
//!
//!    is exactly that, and it is what every one of the twelve programs hit at
//!    its first `.result`. The hoist now happens at the outermost application
//!    of the chain, and the `name$default$n` arguments are re-pointed at the
//!    local wherever in the chain they sit.
//!
//! 2. **`asInstanceOf` / `isInstanceOf` on a primitive qualifier.**
//!    `emit_as_instance_of` reads its receiver as an `Object` -- which is what
//!    `Any` erases to -- so an `int` on the stack is either a `VerifyError` or
//!    an `intValue()` on something that was never boxed. nsc's erasure settles
//!    this before the cast exists: primitive-to-primitive is a numeric
//!    conversion (`i.asInstanceOf[Long]` is `i2l`, and to the same type it is
//!    nothing), primitive-to-reference is a box. slick's
//!    `StatementInvoker.iteratorTo` is
//!    `results(maxRows).fold(r => new CloseableIterator.Single[R](r.asInstanceOf[R]), identity)`
//!    over an `Either[Int, …]`.
//!
//! Self-contained (own copies of the small helpers `e2e.rs` also has) per
//! `.agent-brief.md`: the shared test files belong to other in-flight agents.

use std::fs;
use std::path::PathBuf;
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
        "scala-rs-selfrec-{tag}-{}-{nanos}-{seq}",
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
            fixtures_dir().join("selfrec.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        res.status.success(),
        "scala-rs failed on selfrec.scala:\n{}\n{}",
        String::from_utf8_lossy(&res.stdout),
        String::from_utf8_lossy(&res.stderr)
    );
    Some(out)
}

/// The whole fixture, run under the bytecode verifier, against real scalac
/// 2.13.16's own stdout for the same file. The `recv=1` lines are the point of
/// the receiver hoist: the qualifier of a call that omits a default is
/// evaluated exactly once, however many defaults and clauses it has.
#[test]
fn fixtures_selfrec_matches_scalac() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip selfrec: no scala-library jar");
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
        "selfrec failed at run time:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let expected = fs::read_to_string(fixtures_dir().join("expected").join("selfrec.txt")).unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = fs::remove_dir_all(&out);
}

fn javap(out: &PathBuf, class: &str) -> Option<String> {
    let res = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .ok()?;
    res.status
        .success()
        .then(|| String::from_utf8_lossy(&res.stdout).into_owned())
}

/// No `unresolved apply` may survive anywhere in the fixture. Stdout alone
/// cannot see this: the throw sits on a path a passing run may not take.
#[test]
fn fixtures_selfrec_has_no_unresolved_apply() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip selfrec: no scala-library jar");
        return;
    };
    for class in ["Main$", "Invoker", "IntInvoker", "Casts"] {
        let Some(text) = javap(&out, class) else {
            continue;
        };
        assert!(
            !text.contains("unresolved apply"),
            "{class} still carries an unresolved apply:\n{text}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// `i.asInstanceOf[Long]` is a conversion, not a cast, and a primitive cast to
/// a reference is a box -- both of which `javap` can see and stdout cannot
/// distinguish from an accidental round trip through the wrong wrapper.
#[test]
fn fixtures_selfrec_primitive_casts_box_and_convert() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip selfrec: no scala-library jar");
        return;
    };
    let text = javap(&out, "Casts").expect("javap Casts");
    let body = |name: &str| -> String {
        let start = text
            .find(&format!(" {name}("))
            .unwrap_or_else(|| panic!("no method {name} in:\n{text}"));
        let rest = &text[start..];
        let end = rest[1..].find("\n\n").map(|i| i + 1).unwrap_or(rest.len());
        rest[..end].to_string()
    };
    // Primitive -> abstract type parameter: boxed, no cast on the primitive.
    let to_abstract = body("toAbstract");
    assert!(
        to_abstract.contains("Integer.valueOf"),
        "toAbstract did not box:\n{to_abstract}"
    );
    // Primitive -> the same primitive: nothing at all.
    let to_same = body("toSame");
    assert!(
        !to_same.contains("checkcast") && !to_same.contains("intValue"),
        "toSame is not a no-op:\n{to_same}"
    );
    // Primitive -> another primitive: a numeric conversion.
    let to_long = body("toLong");
    assert!(
        to_long.contains("i2l") && !to_long.contains("checkcast"),
        "toLong is not an i2l:\n{to_long}"
    );
    let to_byte = body("toByte");
    assert!(to_byte.contains("i2b"), "toByte is not an i2b:\n{to_byte}");
    // Reference -> primitive still unboxes.
    let from_any = body("fromAny");
    assert!(
        from_any.contains("intValue"),
        "fromAny does not unbox:\n{from_any}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A value class's `name$default$n$extension` has to sit on the *companion
/// module* as well as on the class. Only a separately compiled caller links
/// against that copy, so running the fixture cannot see it: slick's
/// `StringColumnExtensionMethods.like(pattern)` is called from a program real
/// scalac compiled, and got `NoSuchMethodError:
/// StringColumnExtensionMethods$.like$default$2$extension`. These are the
/// descriptors scalac 2.13.16 emits for the same fixture.
#[test]
fn fixtures_selfrec_value_class_default_getters_are_on_the_module() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip selfrec: no scala-library jar");
        return;
    };
    let text = javap(&out, "Ops$").expect("javap Ops$");
    for want in [
        "like$extension(java.lang.String, java.lang.String, char)",
        "like$default$2$extension(java.lang.String)",
        "twice$default$1$extension(java.lang.String)",
    ] {
        assert!(text.contains(want), "Ops$ has no {want}:\n{text}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// The wide descriptor of a member overridden at a narrower parameter type is
/// a bridge, not a second mixin forwarder to the base trait's body. `javap` is
/// the check that matters: a forwarder to the base would still *link*, and a
/// caller holding the derived static type never reaches it -- slick calls
/// `openStream` through `SynchronousDatabaseAction`, and got the base's
/// `throw new SlickException("Streaming is not supported by this Action")`.
#[test]
fn fixtures_selfrec_narrowed_override_gets_a_bridge() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip selfrec: no scala-library jar");
        return;
    };
    let text = javap(&out, "StreamImpl").expect("javap StreamImpl");
    for (sig, want) in [
        ("public java.lang.String open(Ctx)", "invokevirtual"),
        ("public int size(Ctx)", "invokevirtual"),
    ] {
        let start = text
            .find(sig)
            .unwrap_or_else(|| panic!("no {sig} in:\n{text}"));
        let rest = &text[start..];
        let end = rest[1..].find("\n\n").map(|i| i + 1).unwrap_or(rest.len());
        let body = &rest[..end];
        assert!(
            body.contains(want)
                && body.contains("checkcast")
                && !body.contains("StreamBase.open$")
                && !body.contains("StreamBase.size$"),
            "{sig} is not a bridge to the override:\n{body}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// `f.asInstanceOf[A => B](v)` applies the *cast*, not `asInstanceOf`. The
/// backend used to strip the `TypeApply` and call `asInstanceOf` with the
/// argument (`NoSuchMethodError: java.lang.Object.asInstanceOf()`).
#[test]
fn fixtures_selfrec_applied_cast_calls_the_function() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip selfrec: no scala-library jar");
        return;
    };
    let text = javap(&out, "Main$").expect("javap Main$");
    assert!(
        !text.contains("asInstanceOf"),
        "an asInstanceOf was emitted as a call:\n{text}"
    );
    assert!(
        text.contains("scala/Function1.apply") && text.contains("scala/Function2.apply"),
        "the cast function was not applied:\n{text}"
    );
    let _ = fs::remove_dir_all(&out);
}
