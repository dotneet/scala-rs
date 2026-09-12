//! gitbucket's last compile error, the wrong acceptance under it, and the two
//! backend/typer defects that only became visible once the typer stopped
//! erroring on `Repository.scala`.
//!
//! * **A mirrored companion member is not inherited** (`gz2_repstatic_bad`,
//!   `gz2_javastatic_bad`, `gz2_statlib` + `gz2_statuse*`). scalac mirrors every
//!   accessible companion-object member onto the class's own class file as a
//!   `static` forwarder, and `classpath::fill_java_members` installs it like any
//!   other member; `enter_inherited_members` then put it in the unqualified
//!   scope of every subclass. So slick's `object Rep`'s `Some` won
//!   `.shaped.<>(…, r => Some(…))` inside gitbucket's `Repository` table over
//!   `scala.Some` -- a slick table is a subclass of `Rep` through
//!   `AbstractTable`. nsc's rule is that static Java members belong to
//!   companion objects in Scala and are not inherited;
//!   `check_overload::not_inherited_static` already said so for a *selection*
//!   (`docs/gitbucket.md` root 26) and now says it for the unqualified name too.
//!   Selecting one on the companion, on the exact class that declares it, and
//!   importing it all still work, for a Scala companion and a real Java class
//!   alike.
//! * **A super accessor for a default getter** (`gz2_superdefault`).
//!   `super.m(x)` on a method with a defaulted parameter also needs
//!   `super.m$default$2`, and the getter is a synthesized *symbol* carrying its
//!   body in `Symbol::default_rhs`, never a `DefDef` in the trait's body --
//!   so `next_lin_impl` never found it and the class got
//!   `throw new RuntimeException("no super implementation for m$default$2")`
//!   plus an emit error. Fourteen gitbucket controllers reach this through
//!   `RequestCache`'s `super.getAccountByUserName(userName)`.
//! * **An anonymous class passed to a repeated parameter**
//!   (`gz2_varargs_anon`). A repeated parameter's field symbol has the type it
//!   has inside the body (`Seq[T]`), which is no single argument's expected
//!   type; handing it out made the argument fail, `type_apply_in` roll it back
//!   and type it again, and the second pass re-enter the template's members with
//!   fresh symbols whose signatures the node-id-keyed `sig_done` then skipped.
//! * **A parent constructor argument the signature pass could not type**
//!   (`gz2_convuse` + `gz2_convlib`). `type_local_template` completes an
//!   anonymous class's bodies where it stands and the later passes do not
//!   revisit them, so the signature pass froze a body typed before a unit that
//!   comes later on the command line had any signatures -- with its diagnostic
//!   dropped, leaving an `Error` type that only the backend noticed.
//!
//! Every fixture is also compiled by scalac 2.13.16, which must agree, and the
//! ones that run print the same bytes under both compilers.
//! Fixture prefix: `gz2_`.

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

fn src(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-gz2-{tag}-{}-{nanos}-{seq}",
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

/// slick 3.4.1 and what it needs at compile time, plus `scala-reflect.jar`:
/// slick's `TableQuery` is a macro, and running a macro implementation needs
/// `scala.reflect.runtime.universe` (real scalac has it on the *compiler's*
/// classpath, never on the project's -- the same reason
/// `tests/slick_measure.sh` appends it).
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

fn compile(c: Compiler, srcs: &[PathBuf], out: &Path, cp: Option<&str>) -> Output {
    let jar = scala_library_jar().unwrap();
    match c {
        Compiler::Scalac(sc) => {
            let mut classpath = jar.display().to_string();
            if let Some(cp) = cp {
                classpath = format!("{classpath}:{cp}");
            }
            Command::new(sc)
                .args(["-classpath", &classpath, "-d", out.to_str().unwrap()])
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
            cmd.output().expect("run scala-rs compile")
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

/// The lines a compiler reported as errors, for scalac's format and scala-rs's
/// alike. Only the *lines* are compared: the two word their messages
/// differently and always have.
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

/// Every source compiles under both compilers (nothing is run).
fn check_compiles_both(srcs: &[&str], cp: Option<&str>) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let paths: Vec<PathBuf> = srcs.iter().map(|n| src(n)).collect();
    for c in [Compiler::ScalaRs, Compiler::Scalac(&sc)] {
        let dir = tmp_dir(srcs[0]);
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let o = compile(c, &paths, &out, cp);
        assert!(
            o.status.success(),
            "{} must compile:\n{}",
            srcs[0],
            text(&o)
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

/// Both compilers reject `name`, on the same lines.
fn check_rejects_both(name: &str, cp: Option<&str>) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let file = format!("{name}.scala");
    let mut seen: Vec<Vec<u32>> = Vec::new();
    for c in [Compiler::ScalaRs, Compiler::Scalac(&sc)] {
        let dir = tmp_dir(name);
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let o = compile(c, &[src(name)], &out, cp);
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

/// The sources compile, in the order given, and print
/// `expected/<srcs[0]>.txt` under both compilers.
fn check_runs_both(srcs: &[&str], main: &str, cp: Option<&str>) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let paths: Vec<PathBuf> = srcs.iter().map(|n| src(n)).collect();
    let expected = fs::read_to_string(
        fixtures_dir()
            .join("expected")
            .join(format!("{}.txt", srcs[0])),
    )
    .unwrap();
    for c in [Compiler::ScalaRs, Compiler::Scalac(&sc)] {
        let dir = tmp_dir(srcs[0]);
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let o = compile(c, &paths, &out, cp);
        assert!(
            o.status.success(),
            "{} must compile:\n{}",
            srcs[0],
            text(&o)
        );
        assert_eq!(
            run_java(&out, cp, main),
            expected,
            "{}: output differs",
            srcs[0]
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

fn with_jar(f: impl FnOnce()) {
    if scala_library_jar().is_none() || scalac().is_none() {
        eprintln!("skip: scalac or the scala-library jar not present");
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

// ------------------------------- a companion's static mirror is not inherited

/// gitbucket `Repository.scala:77`: the `Some(...)` of `.shaped.<>(…)` inside a
/// slick `Table` is `scala.Some`, not `slick.lifted.Rep.Some`.
#[test]
fn gz2_repsome_picks_scala_some_inside_a_table() {
    with_slick(|cp| check_compiles_both(&["gz2_repsome"], Some(cp)));
}

/// The wrong acceptance under it, reduced to four lines.
#[test]
fn gz2_repstatic_bad_does_not_inherit_object_reps_members() {
    with_slick(|cp| check_rejects_both("gz2_repstatic_bad", Some(cp)));
}

/// The same rule for a real Java class's statics, which Java inherits and Scala
/// does not.
#[test]
fn gz2_javastatic_bad_does_not_inherit_thread_statics() {
    with_jar(|| check_rejects_both("gz2_javastatic_bad", None));
}

/// `gz2_statlib` is compiled by **real scalac** into a directory, so the client
/// reads `Gz2Base.class` exactly as it would read a jar: with
/// `public static java.lang.String mk(int)` on it. Every legitimate way to reach
/// such a member still works, and the program runs.
#[test]
fn gz2_statuse_reaches_a_mirrored_member_every_legal_way() {
    let (Some(_), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip: scalac or the scala-library jar not present");
        return;
    };
    let dir = tmp_dir("statlib");
    let lib = dir.join("lib");
    fs::create_dir_all(&lib).unwrap();
    let o = compile(Compiler::Scalac(&sc), &[src("gz2_statlib")], &lib, None);
    assert!(o.status.success(), "library must compile:\n{}", text(&o));
    let cp = lib.display().to_string();
    check_runs_both(&["gz2_statuse"], "Gz2StatUse", Some(cp.as_str()));
    check_rejects_both("gz2_statuse_bad", Some(cp.as_str()));
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------ a super accessor for a default getter

#[test]
fn gz2_superdefault_forwards_a_default_getter_through_super() {
    with_jar(|| check_runs_both(&["gz2_superdefault"], "Gz2SuperDefault", None));
}

// ------------------------------------ a template inside an argument position

#[test]
fn gz2_varargs_anon_types_an_anonymous_class_in_a_repeated_parameter() {
    with_jar(|| check_runs_both(&["gz2_varargs_anon"], "Gz2VarargsAnon", None));
}

/// The client comes **first** on the command line, so the conversion it needs
/// has no signatures while the signature pass walks its parent clause.
#[test]
fn gz2_convuse_retypes_a_parent_argument_the_signature_pass_could_not() {
    with_jar(|| check_runs_both(&["gz2_convuse", "gz2_convlib"], "Gz2ConvUse", None));
}
