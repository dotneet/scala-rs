//! Mixin forwarders and outer/super accessors: programs that compiled and
//! then ran wrong.
//!
//! * `mixcg_binmixin` -- traits read from the scala-library jar
//!   (`NoStackTrace`, `SeqOps`'s `super.sizeCompare`, a stackable trait over
//!   `ArrayBuffer`) and the cast a trait-typed value needs where the class the
//!   trait extends is expected.
//! * `mixcg_jaruse` -- the same for a user library packed into a jar.
//! * `mixcg_outer` -- `Q.super.m` from a class nested in `Q`, `new p.C {}`
//!   holding `p` as the enclosing instance, inner case class equality, and
//!   the members a parent constructor's arguments may see.
//! * `mixcg_parentargs_bad` -- the template's own members, rejected there.
//!
//! Every fixture is also compiled by scalac 2.13.16 and must print the same.
//!
//! Fixture prefix: `mixcg_`.

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
        "scala-rs-mixcg-{tag}-{}-{nanos}-{seq}",
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

fn jar_tool() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(home).join("bin/jar");
        if p.is_file() {
            return Some(p);
        }
    }
    let out = Command::new("which").arg("jar").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    p.is_file().then_some(p)
}

/// Compile `srcs` into `out` with scalac (`scalac_path`) or scala-rs, with
/// `cp` (the scala-library jar included) on the classpath.
fn compile(scalac_path: Option<&Path>, srcs: &[PathBuf], out: &Path, cp: &str) -> Output {
    fs::create_dir_all(out).unwrap();
    match scalac_path {
        Some(sc) => Command::new(sc)
            .args(["-classpath", cp, "-d", out.to_str().unwrap()])
            .args(srcs)
            .output()
            .expect("run scalac"),
        None => {
            let jar = scala_library_jar().unwrap();
            let mut c = Command::new(bin());
            c.arg("compile").args(srcs);
            c.args(["-d", out.to_str().unwrap()]);
            let extra: Vec<&str> = cp
                .split(':')
                .filter(|p| *p != jar.to_str().unwrap())
                .collect();
            if !extra.is_empty() {
                c.args(["-cp", &extra.join(":")]);
            }
            c.args(["--scala-library", jar.to_str().unwrap()]);
            c.output().expect("run scala-rs compile")
        }
    }
}

fn assert_ok(o: &Output, what: &str) {
    assert!(
        o.status.success(),
        "{what} failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

fn run_java(out: &Path, cp: &str) -> String {
    let cp = format!("{}:{cp}", out.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert_ok(&o, "java Main");
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// Compile the fixture `name` with `compiler`, run it, and compare with the
/// expected output.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let jar = jar.to_str().unwrap().to_string();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    let src = fixtures_dir().join(format!("{name}.scala"));
    assert_ok(&compile(scalac_path, &[src], &out, &jar), "compile");
    assert_eq!(run_java(&out, &jar), expected(name));
    let _ = fs::remove_dir_all(&dir);
}

/// `mixcg_jaruse` against `mixcg_jarlib` compiled by scalac and packed into a
/// jar; the user side is compiled by `compiler`.
fn check_jar_use(scalac_path: Option<&Path>) {
    let (Some(jar), Some(sc), Some(tool)) = (scala_library_jar(), scalac(), jar_tool()) else {
        eprintln!("skip: scala-library jar, scalac or jar tool not present");
        return;
    };
    let jar = jar.to_str().unwrap().to_string();
    let dir = tmp_dir("jaruse");
    let lib = dir.join("lib");
    let lib_src = fixtures_dir().join("mixcg_jarlib/MixcgLib.scala");
    assert_ok(&compile(Some(&sc), &[lib_src], &lib, &jar), "scalac lib");
    let lib_jar = dir.join("lib.jar");
    let o = Command::new(tool)
        .args([
            "cf",
            lib_jar.to_str().unwrap(),
            "-C",
            lib.to_str().unwrap(),
            ".",
        ])
        .output()
        .expect("run jar");
    assert_ok(&o, "jar");
    let cp = format!("{}:{jar}", lib_jar.display());
    let out = dir.join("out");
    let src = fixtures_dir().join("mixcg_jaruse.scala");
    assert_ok(&compile(scalac_path, &[src], &out, &cp), "compile");
    assert_eq!(run_java(&out, &cp), expected("mixcg_jaruse"));
    let _ = fs::remove_dir_all(&dir);
}

/// The diagnostics both compilers must give for `mixcg_parentargs_bad`.
const PARENT_ARG_ERRORS: [&str; 3] = [
    "not found: value a",
    "not found: type Inner",
    "not found: value b",
];

fn check_parent_args_rejected(scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("parentargs");
    let src = fixtures_dir().join("mixcg_parentargs_bad.scala");
    let o = compile(scalac_path, &[src], &dir.join("out"), jar.to_str().unwrap());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!o.status.success(), "accepted:\n{text}");
    for e in PARENT_ARG_ERRORS {
        assert!(text.contains(e), "missing `{e}` in:\n{text}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn mixcg_binary_mixins_and_trait_casts_run() {
    check_runs("mixcg_binmixin", None);
}

#[test]
fn scalac_agrees_mixcg_binary_mixins_and_trait_casts() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("mixcg_binmixin", Some(&sc));
}

#[test]
fn mixcg_traits_from_a_user_jar_run() {
    check_jar_use(None);
}

#[test]
fn scalac_agrees_mixcg_traits_from_a_user_jar() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_jar_use(Some(&sc));
}

#[test]
fn mixcg_outer_and_super_accessors_run() {
    check_runs("mixcg_outer", None);
}

#[test]
fn scalac_agrees_mixcg_outer_and_super_accessors() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("mixcg_outer", Some(&sc));
}

#[test]
fn mixcg_parent_args_do_not_see_template_members() {
    check_parent_args_rejected(None);
}

#[test]
fn scalac_agrees_mixcg_parent_args_do_not_see_template_members() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_parent_args_rejected(Some(&sc));
}

/// `mixcg_tsuper/MixcgTs.scala` compiled by `lib_by` (`None`: scala-rs), and
/// `mixcg_tsuper_use.scala` against it by `use_by`.
fn check_trait_outer_super(lib_by: Option<&Path>, use_by: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let jar = jar.to_str().unwrap().to_string();
    let dir = tmp_dir("tsuper");
    let lib = dir.join("lib");
    let lib_src = fixtures_dir().join("mixcg_tsuper/MixcgTs.scala");
    assert_ok(&compile(lib_by, &[lib_src], &lib, &jar), "compile lib");
    let cp = format!("{}:{jar}", lib.display());
    let out = dir.join("out");
    let src = fixtures_dir().join("mixcg_tsuper_use.scala");
    assert_ok(&compile(use_by, &[src], &out, &cp), "compile use");
    assert_eq!(run_java(&out, &cp), expected("mixcg_tsuper_use"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn mixcg_trait_outer_super_runs() {
    check_trait_outer_super(None, None);
}

#[test]
fn mixcg_trait_outer_super_accessor_is_pickled_for_scalac() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_trait_outer_super(None, Some(&sc));
}

#[test]
fn scalac_agrees_mixcg_trait_outer_super() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_trait_outer_super(Some(&sc), Some(&sc));
}
