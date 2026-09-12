//! Erasure, primitive/value-class and cast codegen: programs that compiled
//! and then ran wrong, crashed, or had a different ABI from scalac's.
//!
//! * `erascg_valueinst`: a library value class's member that only a
//!   universal trait declares (`5.isValidByte` has no `$extension`; nsc
//!   allocates the `RichInt`), a function-typed ascription in call position
//!   (`(F: Int => F)(4)`), a singleton-type pattern binder on an object, and
//!   an extractor taking a `Unit` scrutinee.
//! * `erascg_bound`: a type parameter or abstract type member erases to the
//!   erasure of its upper bound, primitives included (`def id[A <: Int](a:
//!   A): A` is `(I)I`), and an inherited alias read from a nested class is
//!   read through the enclosing class that inherits it (run/t7120b).
//! * `erascg_binlib*`: the same ABI across separate compilation, in both
//!   directions between scalac and scala-rs.
//! * `erascg_views`: an `Array` lambda body under `flatMap`'s prototype is
//!   wrapped by a view (run/t5652), and an eta-expanded repeated parameter is
//!   a `Seq` (run/eta-expand-star).
//! * `erascg_ctor`: `new TreeMap[K, V]` passes its `Ordering` (run/map_test,
//!   run/t2075), `new C` is `new C()` for repeated and overloaded
//!   constructors (run/t8197), a context-bounded class built from an array
//!   (run/t5284), and block-local classes in field initialisers (run/t4171).
//! * `erascg_bad`: the rejections scalac makes that these must keep.
//!
//! Every runnable fixture is also compiled by scalac 2.13.16 and must print
//! the same. Fixture prefix: `erascg_`.

use std::collections::BTreeSet;
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
        "scala-rs-erascg-{tag}-{}-{nanos}-{seq}",
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

#[derive(Clone, Copy)]
enum Compiler<'a> {
    ScalaRs,
    Scalac(&'a Path),
}

fn compile(c: Compiler, src: &Path, out: &Path, cp: Option<&Path>) -> Output {
    let jar = scala_library_jar().unwrap();
    match c {
        Compiler::Scalac(sc) => {
            let mut classpath = jar.display().to_string();
            if let Some(cp) = cp {
                classpath = format!("{classpath}:{}", cp.display());
            }
            Command::new(sc)
                .args(["-classpath", &classpath, "-d", out.to_str().unwrap()])
                .arg(src)
                .output()
                .expect("run scalac")
        }
        Compiler::ScalaRs => {
            let mut cmd = Command::new(bin());
            cmd.arg("compile")
                .arg(src)
                .args(["-d", out.to_str().unwrap()])
                .args(["--scala-library", jar.to_str().unwrap()]);
            if let Some(cp) = cp {
                cmd.args(["-cp", cp.to_str().unwrap()]);
            }
            cmd.output().expect("run scala-rs compile")
        }
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn run_java(out: &Path, cp: Option<&Path>) -> String {
    let jar = scala_library_jar().unwrap();
    let mut classpath = format!("{}:{}", out.display(), jar.display());
    if let Some(cp) = cp {
        classpath = format!("{classpath}:{}", cp.display());
    }
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &classpath, "Main"])
        .output()
        .expect("run java");
    assert!(o.status.success(), "java Main failed:\n{}", text(&o));
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// `name` compiles with `c` (against `cp`, if any) and prints the expected
/// output under `-Xverify:all`.
fn check_runs(name: &str, c: Compiler, cp: Option<&Path>) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let o = compile(c, &src, &out, cp);
    assert!(o.status.success(), "compile failed:\n{}", text(&o));
    assert_eq!(run_java(&out, cp), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// The line numbers of every error, in scalac's and scala-rs's formats.
fn error_lines(t: &str, file: &str) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    let lines: Vec<&str> = t.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        if let Some((_, rest)) = l.split_once(&format!("{file}:")) {
            if let Some((num, _)) = rest.split_once(": error: ") {
                if let Ok(n) = num.parse::<u32>() {
                    out.insert(n);
                }
            }
            continue;
        }
        if l.starts_with("error: ") {
            let at = lines[i + 1..]
                .iter()
                .take(6)
                .find(|x| x.trim_start().starts_with("-->"))
                .copied()
                .unwrap_or("");
            if let Some((_, pos)) = at.split_once(&format!("{file}:")) {
                if let Some(n) = pos.split(':').next().and_then(|n| n.parse::<u32>().ok()) {
                    out.insert(n);
                }
            }
        }
    }
    out
}

fn rejected_lines(name: &str, c: Compiler) -> BTreeSet<u32> {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let o = compile(c, &src, &out, None);
    let t = text(&o);
    assert!(!o.status.success(), "{name} must not compile:\n{t}");
    let _ = fs::remove_dir_all(&dir);
    error_lines(&t, &format!("{name}.scala"))
}

fn with_jar(f: impl FnOnce()) {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    f()
}

fn with_scalac(f: impl FnOnce(&Path)) {
    let (Some(_), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip: scalac or scala-library jar not present");
        return;
    };
    f(&sc)
}

#[test]
fn erascg_valueinst_runs() {
    with_jar(|| check_runs("erascg_valueinst", Compiler::ScalaRs, None));
}

#[test]
fn scalac_agrees_erascg_valueinst() {
    with_scalac(|sc| check_runs("erascg_valueinst", Compiler::Scalac(sc), None));
}

#[test]
fn erascg_bound_erases_to_the_bound() {
    with_jar(|| check_runs("erascg_bound", Compiler::ScalaRs, None));
}

#[test]
fn scalac_agrees_erascg_bound() {
    with_scalac(|sc| check_runs("erascg_bound", Compiler::Scalac(sc), None));
}

#[test]
fn erascg_views_runs() {
    with_jar(|| check_runs("erascg_views", Compiler::ScalaRs, None));
}

#[test]
fn scalac_agrees_erascg_views() {
    with_scalac(|sc| check_runs("erascg_views", Compiler::Scalac(sc), None));
}

#[test]
fn erascg_ctor_runs() {
    with_jar(|| check_runs("erascg_ctor", Compiler::ScalaRs, None));
}

#[test]
fn scalac_agrees_erascg_ctor() {
    with_scalac(|sc| check_runs("erascg_ctor", Compiler::Scalac(sc), None));
}

#[test]
fn erascg_traitcast_casts_interface_values_into_class_slots() {
    with_jar(|| check_runs("erascg_traitcast", Compiler::ScalaRs, None));
}

#[test]
fn scalac_agrees_erascg_traitcast() {
    with_scalac(|sc| check_runs("erascg_traitcast", Compiler::Scalac(sc), None));
}

#[test]
fn erascg_bad_rejected_on_scalacs_lines() {
    with_scalac(|sc| {
        let ours = rejected_lines("erascg_bad", Compiler::ScalaRs);
        let theirs = rejected_lines("erascg_bad", Compiler::Scalac(sc));
        assert_eq!(theirs, BTreeSet::from([12, 13, 14, 15]));
        assert_eq!(ours, theirs);
    });
}

/// The library compiled by either compiler, the client by either compiler:
/// all four combinations link and print the same.
#[test]
fn erascg_binlib_abi_agrees_across_compilers() {
    with_scalac(|sc| {
        let lib_src = fixtures_dir().join("erascg_binlib/ErascgLib.scala");
        for lib_c in [Compiler::Scalac(sc), Compiler::ScalaRs] {
            let lib = tmp_dir("binlib");
            let o = compile(lib_c, &lib_src, &lib, None);
            assert!(o.status.success(), "{}", text(&o));
            check_runs("erascg_binlib_use", Compiler::ScalaRs, Some(&lib));
            check_runs("erascg_binlib_use", Compiler::Scalac(sc), Some(&lib));
            let _ = fs::remove_dir_all(&lib);
        }
    });
}
