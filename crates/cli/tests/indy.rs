//! Function literals lowered to `invokedynamic` + `LambdaMetafactory`
//! (nsc 2.13's `delambdafy:method`), instead of one closure classfile each.
//!
//! Two axes are checked:
//!
//! * **behaviour** — every fixture runs against the private runtime and
//!   against the real scala-library jar, and `indy2` is additionally diffed
//!   against what real scalac 2.13.16 prints;
//! * **shape** — the classfiles a plain `FunctionN` literal produces (none),
//!   what the enclosing class gains instead (a `$anonfun$N` static method and
//!   a `BootstrapMethods` attribute), and which literals still fall back to an
//!   anonymous class (`PartialFunction`, a user-defined SAM type).

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
        "scala-rs-indy-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    None
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

fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Every `*.class` under `out`, as bare file names.
fn class_names(out: &Path) -> Vec<String> {
    let mut v = Vec::new();
    let mut stack = vec![out.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).expect("read output dir") {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().map(|x| x == "class").unwrap_or(false) {
                v.push(p.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    v.sort();
    v
}

fn contains_ascii(path: &Path, needle: &str) -> bool {
    let bytes = fs::read(path).expect("read classfile");
    let text: String = bytes
        .iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' })
        .collect();
    text.contains(needle)
}

#[test]
fn indy1_runs_the_same_in_both_abis() {
    if !java_available() {
        return;
    }
    let exp = expected_stdout("indy1");

    let out = compile_fixture_with("indy1", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        exp,
        "stdout mismatch for indy1 (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip indy1 library run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("indy1", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        exp,
        "stdout mismatch for indy1 (scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Ten `FunctionN` literals across an object, a class and a trait, and not one
/// closure classfile: only `Main`, `Main$`, `Holder`, `Bump` and `Bump$class`.
#[test]
fn indy1_emits_no_closure_classfiles() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip indy1 shape check: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("indy1", &["--scala-library", jar.to_str().unwrap()]);
    let names = class_names(&out);
    assert!(
        !names.iter().any(|n| n.contains("anonfun")),
        "a plain FunctionN literal must not produce a classfile, got {names:?}"
    );
    // The bodies moved onto the classes that lexically contain them.
    assert!(
        contains_ascii(&out.join("Main$.class"), "$anonfun$"),
        "Main$ should carry the hoisted lambda bodies"
    );
    assert!(
        contains_ascii(
            &out.join("Main$.class"),
            "java/lang/invoke/LambdaMetafactory"
        ),
        "Main$ should bootstrap through LambdaMetafactory"
    );
    assert!(
        contains_ascii(&out.join("Bump$class.class"), "$anonfun$"),
        "a lambda inside a trait method belongs to the trait's $class helper"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The `BootstrapMethods` attribute (JVMS 4.7.23) is really written, and
/// `javap` can decode it: one `metafactory` entry per lambda.
#[test]
fn indy1_writes_a_bootstrap_methods_attribute() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip indy1 javap check: jar not obtainable");
        return;
    };
    if !javap_available() {
        return;
    }
    let out = compile_fixture_with("indy1", &["--scala-library", jar.to_str().unwrap()]);
    let output = Command::new("javap")
        .args(["-v", "-p", out.join("Main$.class").to_str().unwrap()])
        .output()
        .expect("javap");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("BootstrapMethods:"),
        "expected a BootstrapMethods attribute, got {text}"
    );
    assert!(
        text.contains("REF_invokeStatic java/lang/invoke/LambdaMetafactory.metafactory"),
        "expected a LambdaMetafactory bootstrap, got {text}"
    );
    assert!(
        text.contains("invokedynamic"),
        "expected an invokedynamic call site, got {text}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime's `scala/Function0` / `scala/Function1` are ordinary
/// interfaces, so `LambdaMetafactory` links against them too — and no
/// scala-library class sneaks in.
#[test]
fn indy1_private_runtime_needs_no_scala_library() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with("indy1", &["--no-scala-library"]);
    assert!(
        out.join("scala/Function1.class").is_file(),
        "private runtime must provide scala/Function1"
    );
    assert!(
        !class_names(&out).iter().any(|n| n.contains("anonfun")),
        "the private ABI lowers plain lambdas through invokedynamic too"
    );
    assert_eq!(run_java(&out, None), expected_stdout("indy1"));
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn indy2_runs_against_the_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip indy2: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("indy2", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout("indy2"),
        "stdout mismatch for indy2"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The recorded expectation is real scalac 2.13.16's own stdout, and ours
/// matches it byte for byte.
#[test]
fn indy2_matches_real_scalac() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip indy2 real-scalac diff: toolchain not obtainable");
        return;
    };
    let src = fixtures_dir().join("indy2.scala");
    let ref_out = tmp_dir("indy2-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile indy2");
    let ref_cp = format!("{}:{}", ref_out.display(), jar.display());
    let reference = Command::new("java")
        .args(["-cp", &ref_cp, "Main"])
        .output()
        .expect("java (real scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let reference = String::from_utf8_lossy(&reference.stdout).to_string();
    assert_eq!(
        reference,
        expected_stdout("indy2"),
        "recorded expectation for indy2 does not match real scalac"
    );

    let out = compile_fixture_with("indy2", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        reference,
        "stdout differs from real scalac for indy2"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// What is *not* an `invokedynamic` yet, stated as a test so the boundary
/// moves deliberately: a `PartialFunction` literal (two abstract methods, so
/// not a SAM — nsc emits a class here too) and a user-defined SAM type.
#[test]
fn indy2_falls_back_to_a_class_for_partial_functions_and_sam_types() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip indy2 shape check: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("indy2", &["--scala-library", jar.to_str().unwrap()]);
    let closures: Vec<String> = class_names(&out)
        .into_iter()
        .filter(|n| n.contains("anonfun"))
        .collect();
    // `pf`, the `collect { case … }` argument, and the `Transform` SAM.
    assert_eq!(
        closures.len(),
        3,
        "expected exactly the two PartialFunctions and the SAM literal, got {closures:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A two-parameter literal is not a `Function1`: the typer rejects it rather
/// than letting codegen build a call site that could never link.
#[test]
fn indy1_bad_arity_is_an_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip indy1_bad: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("indy1_bad.scala");
    let out = tmp_dir("indy1_bad");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(!output.status.success(), "expected indy1_bad to fail");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains("type mismatch; found: (Int, Int) => Int"),
        "expected an arity mismatch diagnostic, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}
