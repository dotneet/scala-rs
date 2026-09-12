//! The last of gitbucket's errors, and the language rules under them.
//!
//! * **An abstract type member of a jar's cake, rebound through the prefix**
//!   (`gzero_basecolumn*`). slick's `trait RelationalProfile { self => trait
//!   API { type BaseColumnType[T] = self.BaseColumnType[T] } }` writes its
//!   alias against the *enclosing* class's `this`, which a pickle records
//!   beside the member and not in the type. nsc's `Types.rebind` replaces the
//!   deferred member it arrives as by the definition the prefix's profile has
//!   (`JdbcTypesComponent`'s `JdbcType[T] with BaseTypedType[T]`), and the
//!   same happens for a method signature read through a receiver whose outer
//!   instance an `import profile.api._` names
//!   (`MappedColumnType.base[T, U: BaseColumnType]`).
//! * **A trait's concrete members that its class file calls abstract**
//!   (`gzero_profileobj`, `gzero_traitlib` + `gzero_traitmixin`): a `val` the
//!   trait's `$init$` assigns through a synthetic
//!   `pkg$Owner$_setter_$x_$eq`, and a nested `object` whose accessor the
//!   implementing class owes. Both are concrete in the pickle and
//!   `ACC_ABSTRACT` on the interface, so `object X extends <jar profile>` was
//!   asked to implement fourteen members it inherits; the backend now emits
//!   the field, the getter and the mixin setter for a jar trait's `val`, and
//!   the `N$module` field and lazy accessor for its nested `object`s.
//! * **A view for a conversion's own implicit parameter** (`gzero_tupleview*`,
//!   `gzero_sortby`): SLS 7.2's view request inside the clause of another
//!   conversion, plus solving that conversion's type parameters from a *tuple*
//!   argument. slick's `Ordered.tuple2Ordered` needs all of it, and it is what
//!   gives gitbucket's `sortBy { … => issue.issueId.desc -> commentId }` an
//!   `Ordered` for its pair.
//!
//! Every fixture is also compiled by scalac 2.13.16, which must agree, and the
//! ones that run print the same bytes under both compilers.
//! Fixture prefix: `gzero_`.

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
        "scala-rs-gzero-{tag}-{}-{nanos}-{seq}",
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

/// slick 3.4.1 and what it needs at compile and run time, plus
/// `scala-reflect.jar`: slick's `TableQuery` is a macro, and running a macro
/// implementation needs `scala.reflect.runtime.universe` (real scalac has it on
/// the *compiler's* classpath, never on the project's -- the same reason
/// `tests/slick_measure.sh` and `tests/gitbucket_measure.sh` append it).
fn slick_cp() -> Option<String> {
    let reflect = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    if !reflect.is_file() {
        return None;
    }
    let jars = [
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
        reflect,
    ];

    Some(
        jars.iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(":"),
    )
}

#[derive(Clone, Copy)]
enum Compiler<'a> {
    ScalaRs,
    Scalac(&'a Path),
}

fn compile(c: Compiler, srcs: &[PathBuf], out: &Path, flags: &[&str], cp: Option<&str>) -> Output {
    let jar = scala_library_jar().unwrap();
    match c {
        Compiler::Scalac(sc) => {
            let mut classpath = jar.display().to_string();
            if let Some(cp) = cp {
                classpath = format!("{classpath}:{cp}");
            }
            Command::new(sc)
                .args(["-classpath", &classpath, "-d", out.to_str().unwrap()])
                .args(flags)
                .args(srcs)
                .output()
                .expect("run scalac")
        }
        Compiler::ScalaRs => {
            let mut cmd = Command::new(bin());
            cmd.arg("compile")
                .args(srcs)
                .args(["-d", out.to_str().unwrap()])
                .args(["--scala-library", jar.to_str().unwrap()]);
            if let Some(cp) = cp {
                cmd.args(["-cp", cp]);
            }
            cmd.args(flags).output().expect("run scala-rs compile")
        }
    }
}

fn run_java(out: &Path, cp: Option<&str>, main: &str) -> String {
    let jar = scala_library_jar().unwrap();
    let mut classpath = format!("{}:{}", out.display(), jar.display());
    if let Some(cp) = cp {
        classpath = format!("{classpath}:{cp}");
    }
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &classpath, main])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java {main} failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The lines a compiler reported as errors, as `(line, message)` pairs, for
/// scalac's format and scala-rs's alike. Only the *lines* are compared: the two
/// word their messages differently and always have.
fn error_lines(t: &str, file: &str) -> Vec<u32> {
    let lines: Vec<&str> = t.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(rest) = l.split_once(&format!("{file}:")).map(|(_, r)| r) {
            if let Some((num, _)) = rest.split_once(": error: ") {
                if let Ok(n) = num.parse::<u32>() {
                    out.push(n);
                }
            }
            continue;
        }
        if l.starts_with("error: ") {
            let at = lines[i + 1..]
                .iter()
                .take(8)
                .find(|x| x.trim_start().starts_with("-->"))
                .copied()
                .unwrap_or("");
            if let Some((_, pos)) = at.split_once(&format!("{file}:")) {
                if let Some(n) = pos.split(':').next().and_then(|n| n.parse::<u32>().ok()) {
                    out.push(n);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// `name` compiles with both compilers (nothing is run).
fn check_compiles_both(name: &str, flags: &[&str], cp: Option<&str>) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    for c in [Compiler::ScalaRs, Compiler::Scalac(&sc)] {
        let dir = tmp_dir(name);
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let o = compile(c, std::slice::from_ref(&src), &out, flags, cp);
        assert!(o.status.success(), "{name} must compile:\n{}", text(&o));
        let _ = fs::remove_dir_all(&dir);
    }
}

/// Both compilers reject `name`, on the same lines.
fn check_rejects_both(name: &str, flags: &[&str], cp: Option<&str>) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let file = format!("{name}.scala");
    let mut seen: Vec<Vec<u32>> = Vec::new();
    for c in [Compiler::ScalaRs, Compiler::Scalac(&sc)] {
        let dir = tmp_dir(name);
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let o = compile(c, std::slice::from_ref(&src), &out, flags, cp);
        let t = text(&o);
        assert!(!o.status.success(), "{name} must not compile:\n{t}");
        let lines = error_lines(&t, &file);
        assert!(!lines.is_empty(), "{name}: no error lines found in:\n{t}");
        seen.push(lines);
        let _ = fs::remove_dir_all(&dir);
    }
    assert_eq!(
        seen[0], seen[1],
        "{name}: scala-rs and scalac must reject the same lines"
    );
}

/// `name` compiles and prints `expected/<name>.txt` under both compilers.
fn check_runs_both(name: &str, main: &str, flags: &[&str], cp: Option<&str>) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    for c in [Compiler::ScalaRs, Compiler::Scalac(&sc)] {
        let dir = tmp_dir(name);
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let o = compile(c, std::slice::from_ref(&src), &out, flags, cp);
        assert!(o.status.success(), "{name} must compile:\n{}", text(&o));
        assert_eq!(run_java(&out, cp, main), expected, "{name}: output differs");
        let _ = fs::remove_dir_all(&dir);
    }
}

fn with_jar(f: impl FnOnce()) {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    f()
}

fn with_slick(f: impl FnOnce(&str)) {
    let (Some(_), Some(_), Some(cp)) = (scala_library_jar(), scalac(), slick_cp()) else {
        eprintln!("skip: scalac, the scala-library jar or the slick jars are not present");
        return;
    };
    f(&cp)
}

// ------------------------------------------- the cake's abstract type members

#[test]
fn gzero_basecolumn_rebinds_an_outer_this_alias() {
    with_slick(|cp| check_compiles_both("gzero_basecolumn", &[], Some(cp)));
}

#[test]
fn gzero_basecolumn_bad_still_needs_a_real_column_type() {
    with_slick(|cp| check_rejects_both("gzero_basecolumn_bad", &[], Some(cp)));
}

// --------------------------------------- a jar trait's concrete members

#[test]
fn gzero_profileobj_mixes_in_a_jar_profile_and_runs() {
    with_slick(|cp| check_runs_both("gzero_profileobj", "GzeroProfileObj", &[], Some(cp)));
}

/// `gzero_traitlib` is compiled by **real scalac** into a directory first, so
/// `gzero_traitmixin` sees it exactly as it would see a jar: a class file whose
/// concrete `val`s and nested `object`s are `ACC_ABSTRACT` on the interface.
#[test]
fn gzero_traitmixin_implements_a_binary_traits_vals_and_objects() {
    let (Some(_), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip: scalac or the scala-library jar not present");
        return;
    };
    let dir = tmp_dir("traitmixin");
    let lib = dir.join("lib");
    fs::create_dir_all(&lib).unwrap();
    let o = compile(
        Compiler::Scalac(&sc),
        &[fixtures_dir().join("gzero_traitlib.scala")],
        &lib,
        &[],
        None,
    );
    assert!(o.status.success(), "library must compile:\n{}", text(&o));
    let cp = lib.display().to_string();
    check_runs_both(
        "gzero_traitmixin",
        "GzeroTraitMixin",
        &[],
        Some(cp.as_str()),
    );
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------- views inside a clause

#[test]
fn gzero_tupleview_applies_a_conversion_whose_clause_is_a_view() {
    with_jar(|| check_runs_both("gzero_tupleview", "GzeroTupleView", &[], None));
}

#[test]
fn gzero_tupleview_bad_keeps_the_near_misses_out() {
    with_jar(|| check_rejects_both("gzero_tupleview_bad", &[], None));
}

#[test]
fn gzero_sortby_orders_by_a_tuple_of_orderings() {
    with_slick(|cp| check_compiles_both("gzero_sortby", &[], Some(cp)));
}
