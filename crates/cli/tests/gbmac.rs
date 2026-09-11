//! slick's `(a, b).mapTo[Row]` against row classes this run is compiling,
//! and the pieces it needed. `docs/macros.md` §7.25.
//!
//! `ShapedValue.mapToImpl` interrogates its type argument and returns a tree
//! with a pattern-matching anonymous function, an anonymous subclass of
//! `SimpleFastPathResultConverter` and a `super.getDumpInfo.copy(name = …)`.
//! Every step is checked against real scalac 2.13.16:
//!
//! * `gbmac_mapto` -- the whole thing, run against an in-memory H2 database
//!   under `-Xverify:all`, and its near misses (`gbmac_mapto_bad`), which
//!   both compilers reject;
//! * `gbmac_decls_*` -- what the engine's mirror answers about classes this
//!   run is compiling, flag for flag, and what it refuses by name;
//! * `gbmac_caseinfo_use` -- `mapToImpl`'s opening on such classes, the
//!   call `gbm_bad.scala` used to pin as a refusal;
//! * `gbmac_shapes_*` -- the reply shapes the expansion is built of, without
//!   slick;
//! * `gbmac_typer*` -- three typer repairs the expansion depends on, shown
//!   without the macro;
//! * `gbmac_selfimport` -- gitbucket's component shape (`self: Profile =>
//!   import profile.api._` beside an `object Profile`), whose receivers were
//!   silently wrong.
//!
//! Fixture prefix: `gbmac_`.

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

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-gbmac-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scala_reflect_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// A jar from the local Coursier cache, if it happens to be there. Nothing is
/// downloaded.
fn coursier_jar(rel: &str, prefix: &str, version: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    [
        PathBuf::from(&home).join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        PathBuf::from(&home).join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ]
    .into_iter()
    .map(|root| {
        root.join(rel)
            .join(version)
            .join(format!("{prefix}-{version}.jar"))
    })
    .find(|p| p.is_file())
}

/// slick 3.4.1, what it needs at compile time and at run time, and H2.
fn slick_jars() -> Option<Vec<PathBuf>> {
    Some(vec![
        coursier_jar("com/typesafe/slick/slick_2.13", "slick_2.13", "3.4.1")?,
        coursier_jar("com/typesafe/config", "config", "1.4.9")
            .or_else(|| coursier_jar("com/typesafe/config", "config", "1.4.2"))?,
        coursier_jar("org/slf4j/slf4j-api", "slf4j-api", "2.0.18")
            .or_else(|| coursier_jar("org/slf4j/slf4j-api", "slf4j-api", "1.7.36"))?,
        coursier_jar(
            "org/reactivestreams/reactive-streams",
            "reactive-streams",
            "1.0.4",
        )?,
        coursier_jar(
            "org/scala-lang/modules/scala-collection-compat_2.13",
            "scala-collection-compat_2.13",
            "2.8.1",
        )?,
        coursier_jar("com/h2database/h2", "h2", "2.4.240")?,
    ])
}

fn join(paths: &[&Path]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

fn diagnostics(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Everything a test here needs, or `None` with a note on stderr.
struct Env {
    lib: PathBuf,
    reflect: PathBuf,
    scalac: PathBuf,
}

fn env(tag: &str) -> Option<Env> {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("skip {tag}: java / javac not available");
        return None;
    }
    let (Some(lib), Some(reflect), Some(scalac)) =
        (scala_library_jar(), scala_reflect_jar(), scalac())
    else {
        eprintln!("skip {tag}: scala-library / scala-reflect / scalac not present");
        return None;
    };
    Some(Env {
        lib,
        reflect,
        scalac,
    })
}

fn scala_rs(src: &[PathBuf], out: &Path, cp: &str, lib: &Path) -> Output {
    Command::new(bin())
        .arg("compile")
        .args(src)
        .args(["-d", out.to_str().unwrap(), "-cp", cp, "--scala-library"])
        .arg(lib)
        .output()
        .expect("run scala-rs")
}

fn scalac_compile(scalac: &Path, src: &[PathBuf], out: &Path, cp: &str) -> Output {
    Command::new(scalac)
        .args(["-cp", cp, "-d", out.to_str().unwrap()])
        .args(src)
        .output()
        .expect("run scalac")
}

fn run_main(cp: &str, main: &str) -> String {
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, main])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java {main} failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| !l.starts_with("SLF4J"))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Compile `name` against slick with scala-rs or with scalac, run its
/// `main_class` on H2, and compare with the recorded output.
fn slick_program(name: &str, main_class: &str, with_scalac: bool) {
    let Some(env) = env(name) else { return };
    let Some(jars) = slick_jars() else {
        eprintln!("skip {name}: slick 3.4.1 / H2 not in the Coursier cache");
        return;
    };
    let jar_refs: Vec<&Path> = jars.iter().map(|p| p.as_path()).collect();
    let out = tmp_dir(name);
    let mut cp_refs = jar_refs.clone();
    cp_refs.push(&env.reflect);
    let cp = join(&cp_refs);
    let o = if with_scalac {
        scalac_compile(&env.scalac, &[fixture(name)], &out, &join(&jar_refs))
    } else {
        scala_rs(&[fixture(name)], &out, &cp, &env.lib)
    };
    assert!(
        o.status.success(),
        "compile {name} failed:\n{}",
        diagnostics(&o)
    );
    let mut run_cp = vec![out.as_path()];
    run_cp.extend(jar_refs.iter().copied());
    run_cp.push(&env.lib);
    run_cp.push(&env.reflect);
    assert_eq!(run_main(&join(&run_cp), main_class), expected(name));
    let _ = fs::remove_dir_all(&out);
}

/// Compile `name` alone with scala-rs or with scalac, run `main_class` under
/// `-Xverify:all`, and compare with the recorded output.
fn plain_program(name: &str, main_class: &str, with_scalac: bool) {
    let Some(env) = env(name) else { return };
    let out = tmp_dir(name);
    let o = if with_scalac {
        scalac_compile(
            &env.scalac,
            &[fixture(name)],
            &out,
            &env.lib.display().to_string(),
        )
    } else {
        scala_rs(
            &[fixture(name)],
            &out,
            &env.reflect.display().to_string(),
            &env.lib,
        )
    };
    assert!(
        o.status.success(),
        "compile {name} failed:\n{}",
        diagnostics(&o)
    );
    assert_eq!(
        run_main(&join(&[out.as_path(), &env.lib]), main_class),
        expected(name)
    );
    let _ = fs::remove_dir_all(&out);
}

/// gitbucket's `trait X { self: Profile => import profile.api._ }` beside an
/// `object Profile`: the self type's member, a class nested in the component,
/// a view from an object of that member, and an import root shadowed where
/// it is used. Each was a silent miscompile (the object's `profile`, or a
/// cast `this` as the receiver).
#[test]
fn gbmac_selfimport_reads_this_profile() {
    plain_program("gbmac_selfimport", "gbmacsi.Main", false);
}

#[test]
fn scalac_agrees_gbmac_selfimport() {
    plain_program("gbmac_selfimport", "gbmacsi.Main", true);
}

#[test]
fn gbmac_mapto_expands_and_runs_on_h2() {
    slick_program("gbmac_mapto", "gbmacm.Main", false);
}

#[test]
fn scalac_agrees_gbmac_mapto() {
    slick_program("gbmac_mapto", "gbmacm.Main", true);
}

#[test]
fn gbmac_typer_repairs_run() {
    slick_program("gbmac_typer", "Main", false);
}

#[test]
fn scalac_agrees_gbmac_typer() {
    slick_program("gbmac_typer", "Main", true);
}

/// Rejected by both compilers, line by line.
fn both_reject(name: &str, lines: &[u32]) {
    let Some(env) = env(name) else { return };
    let Some(jars) = slick_jars() else {
        eprintln!("skip {name}: slick 3.4.1 not in the Coursier cache");
        return;
    };
    let jar_refs: Vec<&Path> = jars.iter().map(|p| p.as_path()).collect();
    let mut cp_refs = jar_refs.clone();
    cp_refs.push(&env.reflect);
    let out = tmp_dir(name);
    let o = scala_rs(&[fixture(name)], &out, &join(&cp_refs), &env.lib);
    let text = diagnostics(&o);
    assert!(!o.status.success(), "scala-rs accepted {name}:\n{text}");
    let o = scalac_compile(&env.scalac, &[fixture(name)], &out, &join(&jar_refs));
    let nsc = diagnostics(&o);
    assert!(!o.status.success(), "real scalac accepted {name}:\n{nsc}");
    for line in lines {
        assert!(
            text.contains(&format!("{name}.scala:{line}:")),
            "scala-rs reported nothing on line {line} of {name}:\n{text}"
        );
        assert!(
            nsc.contains(&format!("{name}.scala:{line}: error")),
            "real scalac reported nothing on line {line} of {name}:\n{nsc}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// `mapTo` to a class whose fields do not line up with the columns, to one
/// with a column too many, and to a class that is not a case class.
#[test]
fn gbmac_mapto_mismatches_are_rejected() {
    both_reject("gbmac_mapto_bad", &[20, 21, 22]);
}

#[test]
fn gbmac_typer_near_misses_are_rejected() {
    both_reject("gbmac_typer_bad", &[15, 19, 26, 27, 28]);
}

/// A macro implementation compiled first (by `impl_compiler`), then a use
/// site compiled by scala-rs or scalac against it and run.
fn two_stage(
    impl_name: &str,
    use_name: &str,
    main_class: &str,
    scalac_impl: bool,
    scalac_use: bool,
) {
    let Some(env) = env(use_name) else { return };
    let impls = tmp_dir(&format!("{impl_name}-impl"));
    let uses = tmp_dir(&format!("{use_name}-use"));
    let reflect_cp = env.reflect.display().to_string();
    let o = if scalac_impl {
        scalac_compile(&env.scalac, &[fixture(impl_name)], &impls, &reflect_cp)
    } else {
        scala_rs(&[fixture(impl_name)], &impls, &reflect_cp, &env.lib)
    };
    assert!(
        o.status.success(),
        "compile {impl_name} failed:\n{}",
        diagnostics(&o)
    );
    let use_cp = join(&[&impls, &env.reflect]);
    let o = if scalac_use {
        scalac_compile(&env.scalac, &[fixture(use_name)], &uses, &use_cp)
    } else {
        scala_rs(&[fixture(use_name)], &uses, &use_cp, &env.lib)
    };
    assert!(
        o.status.success(),
        "compile {use_name} failed:\n{}",
        diagnostics(&o)
    );
    let cp = join(&[&uses, &impls, &env.lib, &env.reflect]);
    assert_eq!(run_main(&cp, main_class), expected(use_name));
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// Every flag, parameter name, type and companion the mirror reports for a
/// class this run is compiling, against what nsc's typer reports -- with the
/// implementation compiled by scala-rs as well, so the whole pipeline is
/// scala-rs's own.
#[test]
fn gbmac_mirror_answers_like_nsc() {
    two_stage(
        "gbmac_decls_impl",
        "gbmac_decls_use",
        "gbmac.use.Main",
        false,
        false,
    );
}

#[test]
fn scalac_agrees_gbmac_mirror() {
    two_stage(
        "gbmac_decls_impl",
        "gbmac_decls_use",
        "gbmac.use.Main",
        true,
        true,
    );
}

/// What the mirror cannot translate faithfully is refused by name at the
/// call site, never answered with a declaration list missing a member.
#[test]
fn gbmac_mirror_refusals_are_named() {
    let Some(env) = env("gbmac_decls_bad") else {
        return;
    };
    let impls = tmp_dir("decls-bad-impl");
    let uses = tmp_dir("decls-bad-use");
    let reflect_cp = env.reflect.display().to_string();
    let o = scala_rs(
        &[fixture("gbmac_decls_impl")],
        &impls,
        &reflect_cp,
        &env.lib,
    );
    assert!(o.status.success(), "{}", diagnostics(&o));
    let use_cp = join(&[&impls, &env.reflect]);
    let o = scala_rs(&[fixture("gbmac_decls_bad")], &uses, &use_cp, &env.lib);
    let text = diagnostics(&o);
    assert!(
        !o.status.success(),
        "scala-rs accepted gbmac_decls_bad:\n{text}"
    );
    for want in [
        "`Inner` is a nested class, which scala-rs cannot describe to the engine",
        "`Elem` is a type member, which scala-rs cannot describe to the engine",
        "`TwoLists` is a case class with more than one parameter list",
        "`h` is accessible within `bad` only",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    assert_eq!(
        text.matches("macro expansion is not implemented").count(),
        4,
        "every call site must be refused:\n{text}"
    );
    // Real scalac compiles and runs the same file: these are scala-rs's limits.
    let o = scalac_compile(&env.scalac, &[fixture("gbmac_decls_bad")], &uses, &use_cp);
    assert!(
        o.status.success(),
        "real scalac rejected gbmac_decls_bad:\n{}",
        diagnostics(&o)
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// `mapToImpl`'s own opening on case classes this run is compiling.
#[test]
fn gbmac_caseinfo_on_current_run_classes() {
    two_stage("gbm_impl", "gbmac_caseinfo_use", "Main", false, false);
}

#[test]
fn scalac_agrees_gbmac_caseinfo() {
    two_stage("gbm_impl", "gbmac_caseinfo_use", "Main", true, true);
}

/// The reply shapes of the expansion, without slick. The implementation is
/// compiled by real scalac for both use sites.
#[test]
fn gbmac_reply_shapes_rebuild_and_run() {
    two_stage("gbmac_shapes_impl", "gbmac_shapes_use", "Main", true, false);
}

#[test]
fn scalac_agrees_gbmac_reply_shapes() {
    two_stage("gbmac_shapes_impl", "gbmac_shapes_use", "Main", true, true);
}
