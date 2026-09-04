//! E2E tests for the `agent/slickimpl` slice: using slick's DSL *through its
//! published jar*, which is a different measurement from compiling slick's own
//! sources.
//!
//! Five roots, none of which the diagnostics named:
//!
//! 1. **A `@specialized` parent came from the class file.** Specialization runs
//!    after pickling, so `JdbcTypesComponent$JdbcTypes$LongJdbcType`'s class
//!    file says its superclass is `DriverJdbcType$mcJ$sp`, whose own superclass
//!    fixes the parameter to `Object`. `java_parents` read that, so
//!    `longColumnType` was a `BaseTypedType[Any]` and no search for
//!    `BaseTypedType[Long]` could ever succeed. `PickleSupply::attach_parents`
//!    now refines a jar class's parents from its pickle -- which is what nsc
//!    reads -- and replaces a specialized variant rather than sitting next to
//!    it. `PickleSupply::ensure_parents` had to stop requiring the class to
//!    have been *adopted* first, since an implicit candidate is a class the
//!    program has only named.
//!
//! 2. **`fill_java_members` overwrote the `implicit` flag.** `import
//!    profile.api._` supplies `stringColumnType` and its 23 siblings from the
//!    pickle with `Flags::IMPLICIT`; loading the class file of the trait that
//!    declares them afterwards replaced their flags wholesale with the ones
//!    the bytecode can express, which do not include `implicit`. Naming
//!    `Table[…]` as a parent was enough to trigger it, and slick's whole DSL
//!    then reported "could not find implicit value of type TypedType[String]".
//!
//! 3. **An import whose prefix is a `val` of the same template.** The template
//!    is typed imports-first, then signatures, so
//!    `trait Profile { val profile: BlockingJdbcProfile; import
//!    profile.blockingApi._; implicit val dateColumnType: BaseColumnType[…] }`
//!    resolved the import against a `profile` that had no type yet. `sig_done`
//!    made that permanent, `BaseColumnType` was "not found: type", and an
//!    implicit whose type is an error fits **every** implicit search: that is
//!    what gitbucket's ~429 `ambiguous implicit: eventColumnType,
//!    dateColumnType` were. Fixed twice over -- `presig_import_prefixes` for a
//!    prefix named in the same template, and `leave_sig_for_body_pass` for one
//!    that is only settled by another unit's signature pass.
//!
//! 4. **A deferred `val` implemented through a self type.** `trait
//!    ProfileProvider { self: Profile => lazy val profile = … }` mixed in
//!    beside `Profile` implements its `val profile`, but neither trait is
//!    below the other, so `drop_overridden` kept both and
//!    `Profile.profile.blockingApi` was selected on an `<overload …>`.
//!    `check_missing_implementations` said `object creation impossible.` for
//!    the same reason.
//!
//! 5. **A renamed import carried only the term namespace.** `import
//!    slick.jdbc.JdbcBackend.{Database => SlickDatabase, Session}`: a jar's
//!    `type` member leaves no trace in the bytecode, so `Session` was "not a
//!    member of object JdbcBackend" and `SlickDatabase` in type position was
//!    the *factory object's* class.
//!
//! `slickimpl.scala` / `slickimpl_bad.scala` need no jar at all -- roots 3 and
//! 4 are plain language rules. `slickimpl_jar.scala` /
//! `slickimpl_jar_bad.scala` use the real `slick_2.13-3.4.1.jar` from the local
//! Coursier cache and skip when it is not there.

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
        "scala-rs-slickimpl-{tag}-{}-{nanos}-{seq}",
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
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// slick 3.4.1 and the jars it needs on the classpath, from the local Coursier
/// cache if they happen to be there. Nothing is downloaded.
fn slick_jars() -> Option<Vec<PathBuf>> {
    let home = std::env::var("HOME").ok()?;
    let roots = [
        PathBuf::from(&home).join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        PathBuf::from(&home).join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ];
    let wanted = [
        ("com/typesafe/slick/slick_2.13", "slick_2.13", Some("3.4.1")),
        ("com/typesafe/config", "config", None),
        ("org/slf4j/slf4j-api", "slf4j-api", None),
        (
            "org/reactivestreams/reactive-streams",
            "reactive-streams",
            None,
        ),
    ];
    let mut out = Vec::new();
    for (rel, prefix, pin) in wanted {
        let mut found = None;
        for root in &roots {
            let Ok(rd) = fs::read_dir(root.join(rel)) else {
                continue;
            };
            for ent in rd.flatten() {
                let version = ent.file_name().to_string_lossy().into_owned();
                if pin.is_some_and(|p| p != version) {
                    continue;
                }
                let candidate = ent.path().join(format!("{prefix}-{version}.jar"));
                if candidate.is_file() {
                    found = Some(candidate);
                }
            }
        }
        out.push(found?);
    }
    Some(out)
}

fn classpath(jars: &[PathBuf]) -> String {
    jars.iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// Compile one fixture. Answers (success, diagnostics, output directory).
fn compile(name: &str, extra: &[&str]) -> (bool, String, PathBuf) {
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
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), msgs, out)
}

fn run_main(cp: &str, main: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Roots 3 and 4 with no jar in sight: an import through a `val` of the same
/// template, and a deferred `val` implemented by a self-typed trait mixed in
/// beside the one that declares it. Runs, and matches nsc's stdout.
#[test]
fn an_import_through_a_val_of_the_same_template_reaches_the_signatures_below_it() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip slickimpl: scala-library jar not present");
        return;
    };
    let (ok, msgs, out) = compile("slickimpl", &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "slickimpl failed to compile:\n{msgs}");
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        assert_eq!(
            run_main(&cp, "SlickImplMain"),
            expected_stdout("slickimpl"),
            "stdout mismatch"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The far side of both rules: an `override` declaration with no body really
/// does take an implementation away, and a member that does not match
/// implements nothing.
#[test]
fn the_neighbouring_abstract_member_rejections_still_stand() {
    let (ok, msgs, out) = compile("slickimpl_bad", &["--no-scala-library"]);
    assert!(!ok, "expected slickimpl_bad to be rejected, got:\n{msgs}");
    for want in [
        "class C10 needs to be abstract.",
        "No implementation found in a subclass for deferred declaration",
        "object creation impossible.",
        "Missing implementation for member of trait A10:",
    ] {
        assert!(msgs.contains(want), "expected {want:?} in:\n{msgs}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// Roots 1, 2, 3 and 5 against the published slick jar: the column types
/// conform to the type they are declared with, `implicitly[TypedType[String]]`
/// survives a `Table` subclass in the same file, and the whole API resolves
/// through an abstract `val profile`.
#[test]
fn slicks_dsl_resolves_through_its_published_jar() {
    let Some(lib) = scala_library_jar() else {
        eprintln!("skip slickimpl_jar: scala-library jar not present");
        return;
    };
    let Some(jars) = slick_jars() else {
        eprintln!("skip slickimpl_jar: slick 3.4.1 not in the local Coursier cache");
        return;
    };
    let (ok, msgs, out) = compile(
        "slickimpl_jar",
        &[
            "-cp",
            &classpath(&jars),
            "-Xsource:3-cross",
            "--scala-library",
            lib.to_str().unwrap(),
        ],
    );
    assert!(ok, "slickimpl_jar failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    let _ = fs::remove_dir_all(&out);
}

/// Reading the pickled parent has to *narrow* what the class file said.
/// `BaseTypedType` is invariant, so the specialized parent's `Object` must not
/// leak back in, and a type nothing declares a column type for is still not
/// found.
#[test]
fn the_specialized_parents_object_does_not_come_back() {
    let Some(lib) = scala_library_jar() else {
        eprintln!("skip slickimpl_jar_bad: scala-library jar not present");
        return;
    };
    let Some(jars) = slick_jars() else {
        eprintln!("skip slickimpl_jar_bad: slick 3.4.1 not in the local Coursier cache");
        return;
    };
    let (ok, msgs, out) = compile(
        "slickimpl_jar_bad",
        &[
            "-cp",
            &classpath(&jars),
            "-Xsource:3-cross",
            "--scala-library",
            lib.to_str().unwrap(),
        ],
    );
    assert!(
        !ok,
        "expected slickimpl_jar_bad to be rejected, got:\n{msgs}"
    );
    for want in [
        "found: LongJdbcType  required: BaseTypedType[Any]",
        "could not find implicit value of type TypedType[SlickJarBad.type]",
    ] {
        assert!(msgs.contains(want), "expected {want:?} in:\n{msgs}");
    }
    let _ = fs::remove_dir_all(&out);
}

fn real_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn scalac_run(scalac: &Path, name: &str, cp: Option<&str>) -> (bool, String) {
    let out = tmp_dir(name);
    let mut cmd = Command::new(scalac);
    if let Some(cp) = cp {
        cmd.args(["-classpath", cp, "-Xsource:3-cross"]);
    }
    cmd.args([
        "-d",
        out.to_str().unwrap(),
        fixtures_dir()
            .join(format!("{name}.scala"))
            .to_str()
            .unwrap(),
    ]);
    let output = cmd.output().expect("run scalac");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&out);
    (output.status.success(), msgs)
}

/// The dual run: real scalac 2.13.16 accepts both good fixtures and rejects
/// both bad ones, in the same places.
#[test]
fn real_scalac_agrees_on_every_fixture() {
    let Some(scalac) = real_scalac() else {
        eprintln!("skip scalac dual run: /tmp/scala-2.13.16 not present");
        return;
    };
    let (ok, msgs) = scalac_run(&scalac, "slickimpl", None);
    assert!(ok, "scalac rejected slickimpl:\n{msgs}");
    let (ok, msgs) = scalac_run(&scalac, "slickimpl_bad", None);
    assert!(!ok, "scalac accepted slickimpl_bad:\n{msgs}");
    assert!(
        msgs.contains("class C10 needs to be abstract.")
            && msgs.contains("Missing implementation for member of trait A10"),
        "scalac rejected slickimpl_bad for other reasons:\n{msgs}"
    );
    let Some(jars) = slick_jars() else {
        eprintln!("skip the slick half of the dual run: slick 3.4.1 not in the Coursier cache");
        return;
    };
    let cp = classpath(&jars);
    let (ok, msgs) = scalac_run(&scalac, "slickimpl_jar", Some(&cp));
    assert!(ok, "scalac rejected slickimpl_jar:\n{msgs}");
    let (ok, msgs) = scalac_run(&scalac, "slickimpl_jar_bad", Some(&cp));
    assert!(!ok, "scalac accepted slickimpl_jar_bad:\n{msgs}");
    assert!(
        msgs.contains("but trait BaseTypedType is invariant")
            && msgs.contains("could not find implicit value for parameter e"),
        "scalac rejected slickimpl_jar_bad for other reasons:\n{msgs}"
    );
}
