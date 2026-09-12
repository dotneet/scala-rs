//! Type prefixes for inner classes (`crates/typer/src/prefix.rs`).
//!
//! `class Outer[T] { class In }` names one type per enclosing instance:
//! `a.In` and `b.In` differ for two stable `a`, `b: Outer`, `Outer#In` is the
//! supertype of all of them, and the members of `o.In` read `T` from `o`'s
//! type. `Type::Class` carries no prefix, so it rides on the as-seen-from view
//! (`Type::Refined` with `AS_SEEN_FROM_MARK` / `PREFIX_MARK`).
//!
//! `pfx_inner.scala` is accepted and its output compared with scalac's;
//! `pfx_bad.scala` and `pfx_override_bad.scala` are rejected on exactly the
//! lines scalac rejects (the override case is a refchecks error in nsc, which
//! a typer error elsewhere in the file would hide, hence its own fixture).
//!
//! Fixture prefix: `pfx_`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
        "scala-rs-pfx-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn compile(src: &Path, out: &Path, jar: &Path, scalac_path: Option<&Path>) -> Output {
    compile_with(src, out, jar, scalac_path, None)
}

/// `compile`, with an extra class directory on the classpath (a library
/// compiled separately).
fn compile_with(
    src: &Path,
    out: &Path,
    jar: &Path,
    scalac_path: Option<&Path>,
    cp: Option<&Path>,
) -> Output {
    match scalac_path {
        Some(sc) => {
            let mut classpath = jar.to_str().unwrap().to_string();
            if let Some(cp) = cp {
                classpath = format!("{classpath}:{}", cp.display());
            }
            Command::new(sc)
                .args(["-classpath", &classpath, "-d", out.to_str().unwrap()])
                .arg(src)
                .output()
                .expect("run scalac")
        }
        None => {
            let mut c = Command::new(bin());
            c.arg("compile")
                .arg(src)
                .args(["-d", out.to_str().unwrap()])
                .args(["--scala-library", jar.to_str().unwrap()]);
            if let Some(cp) = cp {
                c.args(["-cp", cp.to_str().unwrap()]);
            }
            c.output().expect("run scala-rs compile")
        }
    }
}

fn run_java(out: &Path, jar: &Path) -> String {
    run_java_with(out, jar, None)
}

fn run_java_with(out: &Path, jar: &Path, cp: Option<&Path>) -> String {
    let mut cp_s = format!("{}:{}", out.display(), jar.display());
    if let Some(cp) = cp {
        cp_s = format!("{cp_s}:{}", cp.display());
    }
    let cp = cp_s;
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `name` with `compiler` (scala-rs or scalac), run it, and compare
/// with the expected output.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(&src, &out, &jar, scalac_path);
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// The source lines scala-rs (`  --> file:LINE:COL`) or scalac
/// (`file:LINE: error`) reports an error on.
fn error_lines(name: &str, text: &str, scalac: bool) -> Vec<u32> {
    let file = format!("{name}.scala:");
    let mut lines: Vec<u32> = text
        .lines()
        .filter(|l| {
            if scalac {
                l.contains(": error:")
            } else {
                l.trim_start().starts_with("-->")
            }
        })
        .filter_map(|l| {
            let rest = &l[l.find(&file)? + file.len()..];
            rest.split(':').next()?.parse().ok()
        })
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// Compile `name` and require it to be rejected with errors on exactly
/// `lines`.
fn check_rejects(name: &str, lines: &[u32], scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(&src, &out, &jar, scalac_path);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "compile unexpectedly succeeded:\n{text}"
    );
    assert_eq!(
        error_lines(name, &text, scalac_path.is_some()),
        lines,
        "error lines differ:\n{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// An inner class of a *separately compiled* class (`tests/fixtures/pfx_binlib/`):
/// the library built by scalac and by scala-rs, the client by both, every
/// combination running `new c.D`, a method result read through `c`, and a
/// subclass of `c.D`, and printing what scalac's build prints. The hidden
/// outer slot of the binary constructor is the backend's to fill, not an
/// argument the typer counts.
#[test]
fn pfx_binlib_inner_class_through_a_value_prefix() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let lib_src = fixtures_dir().join("pfx_binlib/PfxLib.scala");
    let client = fixtures_dir().join("pfx_binlib_use.scala");
    let expected = fs::read_to_string(fixtures_dir().join("expected/pfx_binlib_use.txt")).unwrap();
    for lib_by_scalac in [true, false] {
        let lib = tmp_dir("binlib");
        let o = compile_with(
            &lib_src,
            &lib,
            &jar,
            lib_by_scalac.then_some(sc.as_path()),
            None,
        );
        assert!(
            o.status.success(),
            "library failed:\n{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        for client_by_scalac in [false, true] {
            let out = tmp_dir("binlib-use");
            let o = compile_with(
                &client,
                &out,
                &jar,
                client_by_scalac.then_some(sc.as_path()),
                Some(&lib),
            );
            assert!(
                o.status.success(),
                "client failed (lib by scalac: {lib_by_scalac}, client by scalac: {client_by_scalac}):\n{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            assert_eq!(run_java_with(&out, &jar, Some(&lib)), expected);
            let _ = fs::remove_dir_all(&out);
        }
        let _ = fs::remove_dir_all(&lib);
    }
}

const BAD_LINES: &[u32] = &[8, 10, 17, 30, 32, 33, 34, 35, 37, 38];
const OVERRIDE_BAD_LINES: &[u32] = &[11];

#[test]
fn pfx_inner_runs() {
    check_runs("pfx_inner", None);
}

#[test]
fn scalac_agrees_pfx_inner() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("pfx_inner", Some(&sc));
}

#[test]
fn pfx_bad_is_rejected() {
    check_rejects("pfx_bad", BAD_LINES, None);
}

#[test]
fn scalac_agrees_pfx_bad() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("pfx_bad", BAD_LINES, Some(&sc));
}

#[test]
fn pfx_override_bad_is_rejected() {
    check_rejects("pfx_override_bad", OVERRIDE_BAD_LINES, None);
}

#[test]
fn scalac_agrees_pfx_override_bad() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("pfx_override_bad", OVERRIDE_BAD_LINES, Some(&sc));
}
