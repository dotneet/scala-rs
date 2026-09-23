//! E2E tests for the `agent/signature` slice: JVMS §4.7.9 `Signature`
//! attributes, plus the JVMS §4.7.2 `ConstantValue` that `@SerialVersionUID`
//! needs.
//!
//! Before this slice no `Signature` attribute was ever emitted, so every
//! generic member of every class this compiler wrote looked raw to Java —
//! `getGenericInterfaces`, `Method#toGenericString` and `Field#getGenericType`
//! all fell back to the erased shape. `docs/scala-corpus.md` named it the
//! largest remaining root in the corpus's `run` set.
//!
//! The signatures are built in `crates/backend/src/sig.rs`, before the erasure
//! phase rewrites symbol types in place, and attached only when they erase
//! back to the descriptor they sit next to — see that module's header for why
//! a refusal is the right answer whenever the two disagree.
//!
//! `sg_sig.scala`'s expected output was taken from **real scalac 2.13.16**,
//! and both of this compiler's modes reproduce it byte for byte.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.

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
        "scala-rs-signature-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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
        "compile {name} failed extra={extra:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
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

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Private-runtime run (`--no-scala-library`).
#[test]
fn generic_signatures_private_runtime() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with("sg_sig", &["--no-scala-library"]);
    let got = run_java(&out, None, "sg.Main");
    assert_eq!(got, expected_stdout("sg_sig"));
    let _ = fs::remove_dir_all(&out);
}

/// Library-ABI run (`--scala-library <jar>`).
#[test]
fn generic_signatures_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("sg_sig", &["--scala-library", jar_s]);
    let got = run_java(&out, Some(jar_s), "sg.Main");
    assert_eq!(got, expected_stdout("sg_sig"));
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn serial_version_annotation_is_loadable_by_scala_reflection() {
    let Some(library) = scala_library_jar() else {
        return;
    };
    let reflect = Path::new("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    let scalac = Path::new("/tmp/scala-2.13.16/bin/scalac");
    if !reflect.is_file() || !scalac.is_file() || !java_available() {
        return;
    }
    let out = compile_fixture_with("sg_sig", &["--scala-library", library.to_str().unwrap()]);
    let cp = format!(
        "{}:{}:{}",
        out.display(),
        library.display(),
        reflect.display()
    );
    let source = fixtures_dir().join("sg_reflect_serial.scala");
    let compiled = Command::new(scalac)
        .args(["-cp", &cp, "-d"])
        .arg(&out)
        .arg(&source)
        .output()
        .expect("scalac reflection consumer");
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert_eq!(run_java(&out, Some(&cp), "CheckSerial"), "sg.Ser\n");
    let _ = fs::remove_dir_all(out);
}

/// The other half of the claim: a member that says nothing beyond its
/// descriptor gets **no** attribute. A `Signature` on every method would pass
/// the reflection tests above just as well and be pure noise in every class
/// file, so this is checked directly against `javap -v`.
#[test]
fn a_monomorphic_member_carries_no_signature() {
    let Ok(javap) = which_javap() else { return };
    let out = compile_fixture_with("sg_sig", &["--no-scala-library"]);
    let text = Command::new(&javap)
        .args(["-v", "-p", "-cp"])
        .arg(&out)
        .arg("sg.C")
        .output()
        .expect("javap");
    let text = String::from_utf8_lossy(&text.stdout).into_owned();
    // `javap` prints `descriptor:` then the member's other attributes, both
    // indented four spaces; the class's own `Signature` sits at column zero.
    // The member header is no good to match on, because javap prints the
    // *generic* form of a member that has a signature.
    let mut last_desc = String::new();
    let mut signed: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(d) = line.strip_prefix("    descriptor: ") {
            last_desc = d.trim().to_string();
        } else if line.starts_with("    Signature:") {
            signed.push(std::mem::take(&mut last_desc));
        }
    }
    assert!(
        !signed.iter().any(|d| d == "(I)I"),
        "e(int) has no generic information and must carry no Signature:\n{text}"
    );
    assert!(
        signed.iter().any(|d| d == "(Lsg/Wrapper;)I"),
        "a(Wrapper) is generic and must carry a Signature:\n{text}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A user value class has two distinct spellings at this boundary: its
/// underlying representation in a JVM descriptor, and its boxed class in a
/// generic type argument.  Both must survive in the emitted classfile so a
/// Java reflection client sees `Box<Wrapped>` rather than `Box<String>`.
#[test]
fn value_class_identity_is_preserved_in_generic_signatures() {
    let Ok(javap) = which_javap() else { return };
    let out = compile_fixture_with("sg_valueclass", &["--no-scala-library"]);

    let holder = Command::new(&javap)
        .args(["-v", "-p", "-cp"])
        .arg(&out)
        .arg("sgvc.Holder")
        .output()
        .expect("javap Holder");
    assert!(
        holder.status.success(),
        "javap Holder failed: {}",
        String::from_utf8_lossy(&holder.stderr)
    );
    let holder = String::from_utf8_lossy(&holder.stdout);
    assert!(
        holder.contains("Signature:")
            && holder.contains("Lsgvc/Box<Lsgvc/Wrapped;>;")
            && holder.contains("super_class: #")
            && holder.contains("sgvc/Box"),
        "Holder must retain boxed Wrapped in its generic parent Signature:\n{holder}"
    );

    let array_holder = Command::new(&javap)
        .args(["-v", "-p", "-cp"])
        .arg(&out)
        .arg("sgvc.ArrayHolder")
        .output()
        .expect("javap ArrayHolder");
    assert!(
        array_holder.status.success(),
        "javap ArrayHolder failed: {}",
        String::from_utf8_lossy(&array_holder.stderr)
    );
    let array_holder = String::from_utf8_lossy(&array_holder.stdout);
    assert!(
        array_holder.contains("Lsgvc/Box<[Lsgvc/Wrapped;>;")
            && !array_holder.contains("Lsgvc/Box<[I>;"),
        "ArrayHolder must keep a value class boxed in its array Signature:\n{array_holder}"
    );

    let api = Command::new(&javap)
        .args(["-v", "-p", "-cp"])
        .arg(&out)
        .arg("sgvc.Api")
        .output()
        .expect("javap Api");
    assert!(
        api.status.success(),
        "javap Api failed: {}",
        String::from_utf8_lossy(&api.stderr)
    );
    let api = String::from_utf8_lossy(&api.stdout);
    assert!(
        api.contains("descriptor: ()Lsgvc/Box;")
            && api.contains("Lsgvc/Box<Lsgvc/Wrapped;>;")
            && api.contains("Signature:")
            && api.contains("descriptor: (Ljava/lang/String;)Ljava/lang/String;"),
        "Api.wrapped must separate erased descriptor from generic Signature:\n{api}"
    );

    let _ = fs::remove_dir_all(&out);
}

/// A value-class parameter in a user SAM has a boxed `Object` bridge in
/// addition to its underlying-typed implementation.  The bridge is exercised
/// through the interface call here, under the verifier, rather than only
/// checking the generated method descriptors.
#[test]
fn value_class_sam_bridge_runs_with_verifier() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with("vc_sam_lambda", &["--no-scala-library"]);
    let got = run_java(&out, None, "vcsam.Main");
    assert_eq!(got, "true\nx\n");
    let _ = fs::remove_dir_all(&out);
}

/// SAM adaptation must keep arrays of value classes boxed, retain the erased
/// type of generic value classes, and mark the Object-shaped adapter as an
/// actual synthetic bridge for downstream classfile readers.
#[test]
fn value_class_sam_edge_adaptation_and_bridge_flags() {
    if !java_available() {
        return;
    }
    let Ok(javap) = which_javap() else { return };
    let out = compile_fixture_with("vc_sam_edge", &["--no-scala-library"]);
    let got = run_java(&out, None, "vcedge.Main");
    assert_eq!(got, "3\nmapped\nsource\n");

    let mut saw_bridge = false;
    for entry in fs::read_dir(&out).expect("read output directory") {
        let path = entry.expect("output entry").path();
        let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !file.contains("anonfun") || !file.ends_with(".class") {
            continue;
        }
        let class = file.trim_end_matches(".class");
        let text = Command::new(&javap)
            .args(["-v", "-p", "-cp"])
            .arg(&out)
            .arg(class)
            .output()
            .expect("javap SAM lambda");
        assert!(
            text.status.success(),
            "javap {class} failed: {}",
            String::from_utf8_lossy(&text.stderr)
        );
        let text = String::from_utf8_lossy(&text.stdout);
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim().starts_with("descriptor: (Ljava/lang/Object;") {
                let flags = lines.get(i + 1).copied().unwrap_or_default();
                assert!(
                    flags.contains("ACC_BRIDGE") && flags.contains("ACC_SYNTHETIC"),
                    "Object SAM adapter must be a synthetic bridge:\n{text}"
                );
                saw_bridge = true;
            }
        }
    }
    assert!(
        saw_bridge,
        "fixture must emit at least one erased SAM bridge"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `Eq.by(_.value)` is a generic `Function1` adaptation: the generated
/// Object-shaped lambda must first recover the value-class wrapper, then read
/// its underlying reference. This is the same shape as Cats' `ZipVector` law
/// instance, and is checked under `-Xverify:all`.
#[test]
fn value_class_function1_adaptation_runs_with_verifier() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with("vc_function_lambda", &["--no-scala-library"]);
    let got = run_java(&out, None, "vcfn.Main");
    assert_eq!(got, "true\n");
    let _ = fs::remove_dir_all(&out);
}

/// A lambda that both captures an ordinary parameter and constructs a nested
/// case class must retain the enclosing instance before its other captures.
/// This guards the general capture layout used by the value-class SAM path:
/// the nested companion access is an implicit `this` read, not another lambda
/// parameter.
#[test]
fn nested_case_class_capture_runs_with_verifier() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with(
        "lambda_capture_nested",
        &[
            "--scala-library",
            scala_library_jar()
                .expect("scala-library fixture jar")
                .to_str()
                .expect("scala-library path"),
        ],
    );
    let jar = scala_library_jar().expect("scala-library fixture jar");
    let got = run_java(
        &out,
        Some(jar.to_str().expect("scala-library path")),
        "Main",
    );
    assert_eq!(got, "x\n");
    let _ = fs::remove_dir_all(&out);
}

/// Java static references inside a Function1 must not be mistaken for an
/// enclosing-instance read. Otherwise the generated indy call site captures
/// an extra receiver and leaves the collection receiver off the stack.
#[test]
fn java_static_function_does_not_capture_enclosing_receiver() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let jar_s = jar.to_str().expect("scala-library path");
    let out = compile_fixture_with("lambda_java_static", &["--scala-library", jar_s]);
    let got = run_java(&out, Some(jar_s), "Main");
    assert_eq!(got, "1\n2\nshadow\nok\n");
    let _ = fs::remove_dir_all(&out);
}

fn which_javap() -> Result<PathBuf, ()> {
    let p = PathBuf::from("javap");
    match Command::new(&p).arg("-version").output() {
        Ok(o) if o.status.success() => Ok(p),
        _ => Err(()),
    }
}
