//! Two places where scala-rs and nsc disagreed about the *ABI*, both of which
//! are invisible while one compiler builds the whole program.
//!
//! **1. `Null` erased to `java/lang/Object`.** nsc gives it a class of its
//! own exactly as it gives `Nothing` one: `def n: Null` is
//! `()Lscala/runtime/Null$;`, `def take(x: Null)` is
//! `(Lscala/runtime/Null$;)I`, a `val`'s field is `Lscala/runtime/Null$;`, and
//! `List[Null]`'s generic signature is `List<Lscala/runtime/Null$;>`. Only
//! `Array[Null]` is `Object[]`, the same exception `Array[Nothing]` gets.
//! Erasing it to `Object` was self-consistent -- the `Signature` attribute is
//! written from our own descriptors, so nothing contradicted anything -- and
//! every call across the boundary was a `NoSuchMethodError`.
//!
//! **2. A `val` of another compilation unit was read with `getfield`.**
//! scalac makes the backing field `private` and publishes an accessor beside
//! it (`javap -p` on `class Holder(val n: Int)`: `private final int n;` plus
//! `public int n();`). Field access control is checked at *resolution*, not
//! at verification, so `getfield` on it passes the JVM verifier and throws
//! `IllegalAccessError` the first time the method runs -- and scala-rs emits
//! its own fields public, so compiling both halves here never showed it.
//!
//! The asymmetry in (2) is real and is asserted below: our class files are
//! readable by scalac either way, because scalac never reaches for the field.
//! Only the direction "scala-rs reads scalac's class files" was broken.
//!
//! Every test that matters here *runs* the result: neither defect is visible
//! to `javap`, to the class-file lint, or to the loader check in
//! `tests/slick_subset.sh`, which does not link method bodies.

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

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-nullcross-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// The jar and scalac together, or `None` with a loud skip: every interop
/// test here needs both.
fn interop_tools() -> Option<(PathBuf, PathBuf)> {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip nx_ interop: needs the scala-library jar and scalac 2.13.16");
        return None;
    };
    java_available().then_some((jar, scalac))
}

fn compile_ours(names: &[&str], out: &Path, extra: &[&str]) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for n in names {
        cmd.arg(fixture(n));
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "scala-rs compile {names:?} extra={extra:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn run_scalac(scalac: &Path, args: &[&str]) {
    let output = Command::new(scalac)
        .args(args)
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac {args:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn compile_nsc(scalac: &Path, names: &[&str], out: &Path, cp: Option<&Path>) {
    let mut args: Vec<String> = Vec::new();
    if let Some(cp) = cp {
        args.push("-cp".into());
        args.push(cp.to_string_lossy().into_owned());
    }
    args.push("-d".into());
    args.push(out.to_string_lossy().into_owned());
    for n in names {
        args.push(fixture(n).to_string_lossy().into_owned());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_scalac(scalac, &refs);
}

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn javap(args: &[&str]) -> String {
    let output = Command::new("javap").args(args).output().expect("javap");
    assert!(
        output.status.success(),
        "javap {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Any `nxlib` field reference in a `javap -c` listing other than an
/// `object`'s `MODULE$` static, which is not a member's storage.
fn field_refs_other_than_module(code: &str) -> bool {
    code.lines()
        .filter(|l| l.contains("Field nxlib/"))
        .any(|l| !l.contains(".MODULE$:"))
}

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// `name descriptor` for every member `javap -s` prints, sorted. Comparing
/// these is comparing the two compilers' erasure, and nothing else: it does
/// not care about member order, and it does not care that scala-rs still
/// emits a `val`'s field public where nsc emits it private (see
/// `nsc_reaches_our_vals_through_the_accessor_either_way`).
fn descriptors(dir: &Path, class: &str) -> Vec<String> {
    let text = javap(&["-p", "-s", "-cp", dir.to_str().unwrap(), class]);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(d) = l.trim().strip_prefix("descriptor: ") else {
            continue;
        };
        let member = lines[i - 1].trim();
        // The member's own name is everything up to the parameter list, minus
        // the modifiers and the (source-level) type in front of it.
        let head = member.split('(').next().unwrap_or(member);
        let name = head.rsplit(' ').next().unwrap_or(head);
        out.push(format!("{} {}", name.trim_end_matches(';'), d));
    }
    out.sort();
    out
}

// ------------------------------------------------------------- Null erasure

/// The whole of `Null`'s erasure, member by member, against real scalac
/// 2.13.16 on the same source. `Array[Null]` is in there as the one position
/// where the answer is `Object[]`.
#[test]
fn nx_null_erasure_matches_scalac() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let ours = tmp_dir("null-ours");
    let theirs = tmp_dir("null-nsc");
    compile_ours(
        &["nx_null"],
        &ours,
        &["--scala-library", jar.to_str().unwrap()],
    );
    compile_nsc(&scalac, &["nx_null"], &theirs, None);

    for class in ["Main$", "Main$Box", "NxNull"] {
        assert_eq!(
            descriptors(&ours, class),
            descriptors(&theirs, class),
            "erasure of {class} differs from scalac's"
        );
    }
    // The bridge down to `Null$`: without the `checkcast` the JVM rejects the
    // class, because `Object` is not assignable to `Null$`. slick's
    // `JdbcTypesComponent$JdbcTypes$NullJdbcType` is the real instance of
    // this, and it is a `VerifyError` rather than anything subtler.
    let bridge = javap(&["-p", "-c", "-cp", ours.to_str().unwrap(), "NxNull"]);
    assert!(
        bridge.contains("checkcast")
            && bridge.contains("class scala/runtime/Null$")
            && bridge.contains("Method lit:(Lscala/runtime/Null$;)Ljava/lang/String;"),
        "the erasure bridge must checkcast to Null$, got:\n{bridge}"
    );
    // Named explicitly, so a regression says which claim broke rather than
    // printing two long lists.
    let m = descriptors(&ours, "Main$").join("\n");
    for want in [
        "n ()Lscala/runtime/Null$;",
        "take (Lscala/runtime/Null$;)I",
        "id (Lscala/runtime/Null$;)Lscala/runtime/Null$;",
        "arr ()[Ljava/lang/Object;",
    ] {
        assert!(m.contains(want), "missing `{want}` in:\n{m}");
    }
    for d in [ours, theirs] {
        let _ = fs::remove_dir_all(d);
    }
}

/// `Null` in a type *argument* reaches the generic signature, which is what
/// `Method#toGenericString` and every reflective reader sees. nsc writes
/// `List<scala.runtime.Null$>`; erasing `Null` to `Object` wrote
/// `List<java.lang.Object>` and the attribute was a claim about a different
/// type.
#[test]
fn nx_null_type_argument_matches_scalac_signature() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let ours = tmp_dir("nullsig-ours");
    let theirs = tmp_dir("nullsig-nsc");
    compile_ours(
        &["nx_lib"],
        &ours,
        &["--scala-library", jar.to_str().unwrap()],
    );
    compile_nsc(&scalac, &["nx_lib"], &theirs, None);

    for dir in [&ours, &theirs] {
        let text = javap(&["-p", "-cp", dir.to_str().unwrap(), "nxlib.NullSig"]);
        assert!(
            text.contains("scala.collection.immutable.List<scala.runtime.Null$> ln();"),
            "`List[Null]` must be `List<Null$>` in {}, got:\n{text}",
            dir.display()
        );
    }
    for d in [ours, theirs] {
        let _ = fs::remove_dir_all(d);
    }
}

/// Both modes run it. The private runtime has to ship a `scala/runtime/Null$`
/// of its own now that descriptors name it -- the verifier resolves a
/// parameter's class even for a method nobody calls.
#[test]
fn nx_null_runs_in_both_modes() {
    if !java_available() {
        return;
    }
    let exp = expected("nx_null");

    let out = tmp_dir("null-priv");
    compile_ours(&["nx_null"], &out, &["--no-scala-library"]);
    assert!(
        out.join("scala/runtime/Null$.class").is_file(),
        "the private runtime must emit scala/runtime/Null$"
    );
    assert_eq!(
        run_main(out.to_str().unwrap()),
        exp,
        "stdout mismatch for nx_null on the private runtime"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip nx_null (jar): scala-library jar not present");
        return;
    };
    let out = tmp_dir("null-jar");
    compile_ours(
        &["nx_null"],
        &out,
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_main(&format!("{}:{}", out.display(), jar.display())),
        exp,
        "stdout mismatch for nx_null against the real scala-library"
    );
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------------------------- mixed runs

/// scalac compiles both halves: the stdout the two mixed runs are compared
/// against.
fn control(scalac: &Path, jar: &Path) -> String {
    let out = tmp_dir("ctl");
    compile_nsc(scalac, &["nx_lib", "nx_app"], &out, None);
    let s = run_main(&format!("{}:{}", out.display(), jar.display()));
    let _ = fs::remove_dir_all(out);
    s
}

/// **scalac compiles the library, scala-rs the application.** The direction
/// that was broken: `h.n` on a `class Holder(val n: Int)` scalac compiled was
/// `getfield fv/Holder.n:I` against a `private` field, which verifies and
/// then throws `IllegalAccessError`.
#[test]
fn nx_our_app_runs_against_nsc_compiled_vals() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let lib = tmp_dir("fwd-lib");
    let app = tmp_dir("fwd-app");
    compile_nsc(&scalac, &["nx_lib"], &lib, None);
    compile_ours(
        &["nx_app"],
        &app,
        &[
            "--scala-library",
            jar.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
        ],
    );

    // Why it runs: the read is a call, and there is no `getfield` of the
    // library's fields anywhere in what we emitted.
    let code = javap(&["-p", "-c", "-cp", app.to_str().unwrap(), "Main$"]);
    assert!(
        code.contains("Method nxlib/Holder.n:()I"),
        "a `val` of a separately compiled class must be read through its \
         accessor, got:\n{code}"
    );
    assert!(
        code.contains("Method nxlib/Holder.c_$eq:(I)V"),
        "a `var` of a separately compiled class must be written through its \
         setter, got:\n{code}"
    );
    // `Store$.MODULE$` is the one legitimate field reference: it is the
    // `object`'s static instance, not a member's storage.
    assert!(
        !field_refs_other_than_module(&code),
        "nothing in nxlib may be reached with getfield/putfield, got:\n{code}"
    );

    assert_eq!(
        run_main(&format!(
            "{}:{}:{}",
            app.display(),
            lib.display(),
            jar.display()
        )),
        control(&scalac, &jar),
        "our application against scalac's library does not behave like \
         scalac's against its own"
    );
    for d in [lib, app] {
        let _ = fs::remove_dir_all(d);
    }
}

/// **scala-rs compiles the library, scalac the application.** The reverse of
/// the pair, and the one that establishes the asymmetry: scalac reads the
/// pickle, where a `val` is a getter, so it calls the accessor and never
/// looks at the field. Our fields being public is why this direction worked
/// even while the other one did not -- it is a difference from nsc's ABI, not
/// a breakage of it.
#[test]
fn nsc_reaches_our_vals_through_the_accessor_either_way() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let lib = tmp_dir("rev-lib");
    let app = tmp_dir("rev-app");
    compile_ours(
        &["nx_lib"],
        &lib,
        &["--scala-library", jar.to_str().unwrap()],
    );
    compile_nsc(&scalac, &["nx_app"], &app, Some(&lib));

    let code = javap(&["-p", "-c", "-cp", app.to_str().unwrap(), "Main$"]);
    assert!(
        code.contains("Method nxlib/Holder.n:()I")
            && code.contains("Method nxlib/Holder.c_$eq:(I)V"),
        "scalac must reach our members through their accessors, got:\n{code}"
    );
    assert!(
        !field_refs_other_than_module(&code),
        "scalac never reads a Scala class's field directly, got:\n{code}"
    );
    // The asymmetry itself, stated where it can be checked: scalac's field is
    // private, ours is not, and the accessor is what makes that survivable.
    // If scala-rs ever narrows the field, this is the line to update.
    let ours = javap(&["-p", "-cp", lib.to_str().unwrap(), "nxlib.Holder"]);
    assert!(
        ours.contains("public int n();"),
        "our accessor must be public, got:\n{ours}"
    );

    assert_eq!(
        run_main(&format!(
            "{}:{}:{}",
            app.display(),
            lib.display(),
            jar.display()
        )),
        control(&scalac, &jar),
        "scalac's application against our library does not behave like \
         scalac's against its own"
    );
    for d in [lib, app] {
        let _ = fs::remove_dir_all(d);
    }
}

/// The same two halves, both compiled by scala-rs on the **private runtime**,
/// where there is no scalac to compare against but the `-cp` reader and the
/// accessor rule are the same code. Catches a fix that only works when the
/// jar's pickles are in play.
#[test]
fn nx_separate_compilation_on_the_private_runtime() {
    if !java_available() {
        return;
    }
    let lib = tmp_dir("priv-lib");
    let app = tmp_dir("priv-app");
    compile_ours(&["nx_lib"], &lib, &["--no-scala-library"]);
    compile_ours(
        &["nx_app"],
        &app,
        &["--no-scala-library", "-cp", lib.to_str().unwrap()],
    );
    assert_eq!(
        run_main(&format!("{}:{}", app.display(), lib.display())),
        expected("nx_app"),
        "separate compilation on the private runtime"
    );
    for d in [lib, app] {
        let _ = fs::remove_dir_all(d);
    }
}

/// Null's ABI class is not a JVM subtype of String. Keep side effects and
/// erased-result cast failures while materializing its sole value, null.
#[test]
fn nx_null_values_preserve_effects_and_casts() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let reference = tmp_dir("values-nsc");
    compile_nsc(&scalac, &["nx_values"], &reference, None);
    let expected = expected("nx_values");
    assert_eq!(
        run_main(&format!("{}:{}", reference.display(), jar.display())),
        expected
    );
    for library in [false, true] {
        let out = tmp_dir("values-rs");
        let args = if library {
            vec!["--scala-library", jar.to_str().unwrap()]
        } else {
            vec!["--no-scala-library"]
        };
        compile_ours(&["nx_values"], &out, &args);
        let cp = if library {
            format!("{}:{}", out.display(), jar.display())
        } else {
            out.to_string_lossy().into_owned()
        };
        assert_eq!(run_main(&cp), expected, "library={library}");
        let code = javap(&["-p", "-c", "-cp", out.to_str().unwrap(), "Main$"]);
        assert!(
            code.lines()
                .any(|line| line.contains("anewarray") && line.contains("class java/lang/Object")),
            "Null arrays must allocate Object[]: {code}"
        );
        let _ = fs::remove_dir_all(out);
    }
    let _ = fs::remove_dir_all(reference);
}

#[test]
fn nx_bottom_array_overloads_collide_after_erasure() {
    let Some((jar, scalac)) = interop_tools() else {
        return;
    };
    let out = tmp_dir("array-overloads");
    let ours = Command::new(bin())
        .arg("compile")
        .arg(fixture("nx_arrays_bad"))
        .args([
            "--scala-library",
            jar.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !ours.status.success(),
        "bottom-array overloads must be rejected"
    );
    let diagnostics = String::from_utf8_lossy(&ours.stderr);
    assert_eq!(
        diagnostics.matches("double definition").count(),
        2,
        "{diagnostics}"
    );
    let theirs = Command::new(scalac)
        .arg(fixture("nx_arrays_bad"))
        .args(["-d", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!theirs.status.success());
    assert_eq!(
        String::from_utf8_lossy(&theirs.stderr)
            .matches("double definition")
            .count(),
        2
    );
    let _ = fs::remove_dir_all(out);
}
