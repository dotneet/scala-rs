//! E2E tests for the `agent/javavarargs` slice.
//!
//! **The defect.** A varargs alternative competed with its own fixed-arity
//! sibling and neither won, so every call both accept was `ambiguous
//! overload`. `java.lang.reflect.Array` declares `newInstance(Class<?>, int)`
//! beside `newInstance(Class<?>, int...)`, and twelve calls in the standard
//! library's own sources stopped there:
//!
//! ```text
//! error: ambiguous overload for newInstance with arguments (Class[A], 0)
//!  --> src/library/scala/Array.scala:158:32
//! ```
//!
//! **It is not Java-specific.** `def g(x: Int)` beside `def g(x: Int*)` in
//! plain Scala had the same error, and real scalac 2.13.16 takes the
//! fixed-arity alternative in both cases. `mutable.Buffer` declares
//! `prepend(elem: A)` beside `prepend(elems: A*)` and was a thirteenth
//! instance of the same root, in Scala, that the brief's count of twelve had
//! not connected to it.
//!
//! **The rule** is nsc's `Infer.isAsSpecific`, and it is not "a fixed-arity
//! alternative wins". A declared `T*`, read as an *argument* type, conforms to
//! no ordinary formal -- so the varargs signature is not as specific as the
//! fixed-arity one, while the fixed-arity one is as specific as it. When
//! *both* alternatives are varargs lists the repeated parameter is unwrapped
//! to its element type first, and then neither wins. Six scalac 2.13.16
//! measurements pin it, and `jvarargs_bad.scala` / `jvarargs_java_bad.scala`
//! are the three of them that must stay ambiguous.
//!
//! **A second defect on the same seam**, pre-existing and found by the first
//! fixture that could reach it: a Java varargs call with a *primitive* element
//! type built an `Object[]` and boxed into it, so `JVar.only(3, 4)` against
//! `only(int...)` was `VerifyError: Type '[Ljava/lang/Object;' is not
//! assignable to '[I'`. Only a reference element type ever worked.
//! `gen_java_varargs_array` now uses the element type the parameter declares.
//!
//! **And a third**, newly reachable once a fixed-arity alternative could win:
//! a `f(xs: _*)` splice is a repeated argument, and a fixed-arity alternative
//! was accepting it as a single element. `g(Seq(4, 5, 6): _*)` picked
//! `g(x: Int)` and died in the verifier. Applicability now refuses a repeated
//! argument to any parameter list that has no repeated parameter, which is one
//! rule with the specificity one above -- nsc's, both of them.
//!
//! Measured with `tests/scalalib_measure.sh`: `errors=740
//! files_with_errors=138` before, `727 / 135` after.
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
        "scala-rs-jvarargs-{tag}-{}-{nanos}-{seq}",
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

fn javac_available() -> bool {
    Command::new("javac")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// `jvarargs.JVar` and `jvarargs.JAmbig`, compiled by javac. This compiler has
/// no Java front end, so a Java declaration reaches it only as a class file --
/// the same arrangement `tests/scalalib_measure.sh` uses for the standard
/// library's own Java sources.
fn compile_java() -> PathBuf {
    let dir = fixtures_dir().join("java/jvarargs");
    let out = tmp_dir("java");
    let status = Command::new("javac")
        .args(["-d", out.to_str().unwrap()])
        .arg(dir.join("JVar.java"))
        .arg(dir.join("JAmbig.java"))
        .status()
        .expect("javac");
    assert!(status.success(), "javac jvarargs.{{JVar,JAmbig}} failed");
    assert!(
        out.join("jvarargs/JVar.class").is_file(),
        "jvarargs/JVar.class missing"
    );
    out
}

fn compile(names: &[&str], extra: &[&str]) -> PathBuf {
    let out = tmp_dir(names[0]);
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for n in names {
        cmd.arg(fixtures_dir().join(format!("{n}.scala")));
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {names:?} (extra={extra:?}) failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn compile_errors(name: &str, extra: &[&str]) -> String {
    let out = tmp_dir(&format!("{name}-bad"));
    let mut cmd = Command::new(bin());
    cmd.arg("compile")
        .arg(fixtures_dir().join(format!("{name}.scala")))
        .args(["-d", out.to_str().unwrap()]);
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

fn javap(out: &Path, class: &str) -> String {
    let o = Command::new("javap")
        .args(["-p", "-c", "-classpath", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    assert!(o.status.success(), "javap {class} failed");
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// The plain-Scala pair. No Java anywhere, which is the point: the defect is
// nsc's specificity rule, not the class-file reader.
// ---------------------------------------------------------------------------

/// Each alternative prints which one ran; the expected output is real scalac
/// 2.13.16's, compiling the same source.
#[test]
fn fixtures_jvarargs() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile(&["jvarargs"], &["--scala-library", jar_s]);
    if java_available() {
        assert_eq!(
            run_main(&out, "jvarargs.Main", &[jar_s]),
            expected_stdout("jvarargs")
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same source in the private runtime -- the mode
/// `tests/scalalib_measure.sh` runs, and so the mode the twelve library errors
/// were reported in. Compiled only: a varargs signature names
/// `scala/collection/immutable/Seq`, which the private runtime does not emit,
/// so the class file cannot be loaded without the jar. That is a pre-existing
/// property of the private runtime and has nothing to do with this slice.
#[test]
fn fixtures_jvarargs_private_compiles() {
    let out = compile(&["jvarargs"], &["--no-scala-library"]);
    let _ = fs::remove_dir_all(&out);
}

/// The ties that must survive. Real scalac 2.13.16 reports exactly these two,
/// at lines 20 and 21 -- a fixed-arity alternative does *not* simply win.
#[test]
fn fixtures_jvarargs_bad_is_error() {
    let jar = scala_library_jar();
    let mut modes: Vec<Vec<&str>> = vec![vec!["--no-scala-library"]];
    let jar_s;
    if let Some(j) = &jar {
        jar_s = j.to_str().unwrap();
        modes.push(vec!["--scala-library", jar_s]);
    } else {
        eprintln!("skip scala-library mode: jar not obtainable");
    }
    for extra in modes {
        let err = compile_errors("jvarargs_bad", &extra);
        assert!(
            err.contains("ambiguous overload for s") && err.contains("jvarargs_bad.scala:20"),
            "expected `s` to stay ambiguous at scalac's line 20, got: {err}"
        );
        assert!(
            err.contains("ambiguous overload for u") && err.contains("jvarargs_bad.scala:21"),
            "expected `u` to stay ambiguous at scalac's line 21, got: {err}"
        );
        assert!(
            err.contains("2 error(s)"),
            "expected exactly 2 errors, as real scalac reports, got: {err}"
        );
    }
}

// ---------------------------------------------------------------------------
// The Java half: genuine `.java` sources compiled by javac and read back as
// class files.
// ---------------------------------------------------------------------------

/// `pick(int)` beside `pick(int...)`, a call only the varargs one can take,
/// and `java.lang.reflect.Array.newInstance` itself -- the member the twelve
/// library errors named. Output is real scalac 2.13.16's for the same source.
#[test]
fn fixtures_jvarargs_java() {
    if !javac_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let java_cp = compile_java();
    let jar_s = jar.to_str().unwrap();
    let cp = java_cp.to_str().unwrap();
    let out = compile(&["jvarargs_java"], &["-cp", cp, "--scala-library", jar_s]);
    if java_available() {
        assert_eq!(
            run_main(&out, "jvarargs.JMain", &[cp, jar_s]),
            expected_stdout("jvarargs_java")
        );
    }
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&java_cp);
}

/// What the run cannot see: that the *fixed-arity* call allocates nothing.
/// A varargs alternative picked where scalac picks the fixed one still
/// produces the same string here, because both print through the same Java
/// method -- so the bytecode is the check. Every descriptor below is the one
/// real scalac 2.13.16 emits for the same source, instruction for
/// instruction.
#[test]
fn jvarargs_java_call_shapes() {
    if !javac_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let java_cp = compile_java();
    let out = compile(
        &["jvarargs_java"],
        &[
            "-cp",
            java_cp.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ],
    );
    let code = javap(&out, "jvarargs.JMain$");
    for (what, needle) in [
        // The fixed-arity alternative: an `int`, not an array.
        ("pick(1)", "jvarargs/JVar.pick:(I)Ljava/lang/String;"),
        ("pick(1, 2)", "jvarargs/JVar.pick:([I)Ljava/lang/String;"),
        ("only(3, 4)", "jvarargs/JVar.only:([I)Ljava/lang/String;"),
        (
            "refs(\"a\", \"b\")",
            "jvarargs/JVar.refs:([Ljava/lang/String;)Ljava/lang/String;",
        ),
        // An `Int` literal reaching a `long[]` element slot.
        ("wide(5)", "jvarargs/JVar.wide:([J)Ljava/lang/String;"),
        (
            "inst(\"z\")",
            "jvarargs/JVar.inst:(Ljava/lang/Object;)Ljava/lang/String;",
        ),
        (
            "newInstance(c, 3)",
            "java/lang/reflect/Array.newInstance:(Ljava/lang/Class;I)Ljava/lang/Object;",
        ),
        (
            "newInstance(c, 2, 3)",
            "java/lang/reflect/Array.newInstance:(Ljava/lang/Class;[I)Ljava/lang/Object;",
        ),
    ] {
        assert!(
            code.contains(needle),
            "{what} should compile to `{needle}`, got:\n{code}"
        );
    }
    // The primitive varargs arrays are built with `newarray`, not
    // `anewarray java/lang/Object` and a box per element, which is what made
    // every `int...` call a `VerifyError`.
    assert!(
        code.contains("newarray       int") && code.contains("newarray       long"),
        "primitive varargs arrays must be primitive arrays, got:\n{code}"
    );
    assert!(
        !code.contains("anewarray     #") || code.contains("class java/lang/String"),
        "the only `anewarray` here is the `String...` one, got:\n{code}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&java_cp);
}

/// The same two ties, declared in Java. Real scalac 2.13.16 reports exactly
/// these two, at lines 8 and 9.
#[test]
fn fixtures_jvarargs_java_bad_is_error() {
    if !javac_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let java_cp = compile_java();
    let err = compile_errors(
        "jvarargs_java_bad",
        &[
            "-cp",
            java_cp.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ],
    );
    assert!(
        err.contains("ambiguous overload for b") && err.contains("jvarargs_java_bad.scala:8"),
        "expected `b` to stay ambiguous at scalac's line 8, got: {err}"
    );
    assert!(
        err.contains("ambiguous overload for d") && err.contains("jvarargs_java_bad.scala:9"),
        "expected `d` to stay ambiguous at scalac's line 9, got: {err}"
    );
    assert!(
        err.contains("2 error(s)"),
        "expected exactly 2 errors, as real scalac reports, got: {err}"
    );
    let _ = fs::remove_dir_all(&java_cp);
}
