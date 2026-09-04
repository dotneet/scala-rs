//! E2E tests for the `agent/lasttwo` slice (fixture prefix `lasttwo`).
//!
//! Four defects. Together they took `tests/slick_run.sh` from `ok=10 diff=1
//! fail=1` to `ok=12 diff=0 fail=0` -- every one of the twelve slick client
//! programs now prints, byte for byte, what the scalac-built slick prints.
//!
//! 1. **A primary constructor's default arguments had no getters.** The typer
//!    splices the stored expression into the call instead, which is invisible
//!    from outside the run: nsc puts `$lessinit$greater$default$n` -- and, for
//!    a case class, `apply$default$n` -- on the companion module, and a
//!    separately compiled caller emits a call to it. slick's `case class
//!    Length(length: Int, varying: Boolean = true)` is reached from client code
//!    as `O.Length(64)`: `NoSuchMethodError:
//!    RelationalProfile$ColumnOption$Length$.apply$default$2` (`p10_types`).
//!    The getter's result type is *inferred* when the parameter's type names
//!    one of the class's type parameters, as nsc's is -- `case class
//!    Comprehension[+Fetch <: Option[Node]](…, fetch: Fetch = None, …)` has no
//!    `None` that conforms to `Fetch`.
//!
//! 2. **A `name$default$n` getter reached through an *inserted* `apply` took
//!    the receiver's qualifier.** `G.H(4)` is `Select(G, "H")` carrying `H`'s
//!    `apply` as its symbol, so the getter call came out as
//!    `G$.apply$default$2` -- a compile error against a program real scalac
//!    accepts. Latent until defect 1 put such getters on companions.
//!
//! 3. **A trait nested in a member `object`, mixed in elsewhere.** Two halves.
//!    The implementing class declined to implement the trait's `$outer`
//!    accessor at all, because the object is not on its own `$outer` chain --
//!    it is reached through the enclosing template's module accessor
//!    (`AbstractMethodError`). And inside the trait's body, members of the
//!    class the trait *extends* were read by walking out to `$outer` instead
//!    of off `this` with a cast: the JVM interface does not extend that class,
//!    but every instance of the trait is one (`ClassCastException`). slick's
//!    `object TableDDLBuilder { trait UniqueIndexAsConstraint extends
//!    TableDDLBuilder }` with `H2Profile`'s `H2TableDDLBuilder` mixing it in.
//!
//! 4. **A mixin forwarder overrode a superclass's own override.** A concrete
//!    trait method whose superclass overrides it at a *narrower* erased
//!    descriptor (an abstract type member fixed further down) is keyed
//!    differently, so the class got a forwarder straight to the trait's body,
//!    which then won over the superclass's method and its bridge. slick's
//!    `abstract class JdbcDatabaseDef.setupTransaction(session: JdbcSessionDef,
//!    …)` was shadowed on `new JdbcDatabaseDef(…){}` by
//!    `BasicDatabaseDef.setupTransaction = None`, so every `.transactionally`
//!    ran with autocommit still on and rolled nothing back (`p06_update_tx`).
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
        "scala-rs-lasttwo-{tag}-{}-{nanos}-{seq}",
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
            fixtures_dir().join("lasttwo.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        res.status.success(),
        "scala-rs failed on lasttwo.scala:\n{}\n{}",
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
fn fixtures_lasttwo_matches_scalac() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip lasttwo: no scala-library jar");
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
        "lasttwo failed at run time:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let expected = fs::read_to_string(fixtures_dir().join("expected").join("lasttwo.txt")).unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = fs::remove_dir_all(&out);
}

/// The constructor defaults' getters, which only a separately compiled caller
/// links against -- stdout cannot see them, because inside one run the default
/// is spliced into the call. Descriptors are nsc's, `scala.None$` included.
#[test]
fn fixtures_lasttwo_ctor_defaults_land_on_the_companion() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip lasttwo: no scala-library jar");
        return;
    };
    for (class, ret) in [
        ("LtLength$", "boolean"),
        ("LtOuter$LtInner$LtNested$", "boolean"),
        // The parameter's type is the class's own `F`, so the result type is
        // the body's, exactly as nsc writes it (`scala.None$`, not `F`).
        ("LtComp$", "scala.None$"),
        // A companion the source wrote gets them just the same.
        ("LtBox$", "java.lang.String"),
    ] {
        let text = javap(&out, class).expect("javap companion");
        for name in ["$lessinit$greater$default$2", "apply$default$2"] {
            let sig = format!("public {ret} {name}();");
            assert!(text.contains(&sig), "{class} has no `{sig}`:\n{text}");
        }
    }
    let _ = fs::remove_dir_all(&out);
}

/// A trait nested in a member `object`: the implementing class reaches the
/// object through the enclosing template's accessor, and the trait's body
/// reads the class it extends off `this`.
#[test]
fn fixtures_lasttwo_nested_object_trait_outer() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip lasttwo: no scala-library jar");
        return;
    };
    let builder = javap(&out, "LtProfile$LtH2Builder").expect("javap builder");
    let acc = method_body(
        &builder,
        "public LtComp2$LtBuilder$ LtComp2$LtBuilder$LtUniqueAsConstraint$$$outer()",
    );
    assert!(
        acc.contains("Field $outer:LLtProfile;")
            && acc.contains("InterfaceMethod LtComp2.LtBuilder:()LLtComp2$LtBuilder$;"),
        "the trait's $outer accessor does not go through the module accessor:\n{acc}"
    );
    let impl_ = javap(&out, "LtComp2$LtBuilder$LtUniqueAsConstraint$class").expect("javap impl");
    let body = method_body(&impl_, "public static java.lang.String index(");
    // Every narrowing to the class the trait extends must sit directly on
    // `this`. Reading it off `$outer` is the shape that threw
    // `ClassCastException`, and offsets move whenever the body does, so this
    // looks at the instruction before each cast rather than at a fixed one.
    let lines: Vec<&str> = body.lines().collect();
    let mut casts = 0;
    for (i, line) in lines.iter().enumerate() {
        if !(line.contains("checkcast") && line.trim_end().ends_with("class LtComp2$LtBuilder")) {
            continue;
        }
        casts += 1;
        let prev = lines[i - 1];
        assert!(
            prev.contains("aload_0"),
            "the trait body narrows something other than `this` to its own \
             class:\n{prev}\n{line}"
        );
    }
    assert!(
        casts >= 2,
        "expected both members to be read off `this`:\n{body}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A class whose superclass already overrides a concrete trait method owes no
/// forwarder: emitting one puts the trait's body back on top of the override.
#[test]
fn fixtures_lasttwo_superclass_override_beats_the_trait_body() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip lasttwo: no scala-library jar");
        return;
    };
    let anon = javap(&out, "Main$$anon$1").expect("javap anon");
    assert!(
        !anon.contains("setup"),
        "the anonymous subclass re-declares the trait method:\n{anon}"
    );
    // The superclass keeps both halves: its own narrowed method and the wide
    // bridge a call through `LtBase` resolves to.
    let mid = javap(&out, "LtMid").expect("javap LtMid");
    let bridge = method_body(&mid, "public java.lang.String setup(java.lang.Object)");
    assert!(
        bridge.contains("Method setup:(Ljava/lang/String;)Ljava/lang/String;"),
        "LtMid's wide setup is not a bridge to the override:\n{bridge}"
    );
    let _ = fs::remove_dir_all(&out);
}
