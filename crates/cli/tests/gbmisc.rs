//! gitbucket's non-macro errors and the language rules under them.
//!
//! * Unapplied methods (`gbmisc_eta*`): in 2.13 an unapplied method with
//!   parameters is a function only where a function type is expected
//!   ("missing argument list" elsewhere); under `-Xsource:3` it is
//!   eta-expanded wherever a value is required, including as a selection
//!   prefix (`RepositoryOptions.apply.tupled`). An eta-expansion evaluates
//!   its receiver once.
//! * A method reference argument against a function or SAM parameter whose
//!   result is a type variable (`xs.foreach(println)`, `gbmisc_fnref*`).
//! * Named arguments over a case class's synthetic `apply` and a written
//!   overload of it (`gbmisc_named_apply*`).
//! * A Java declaration's `Object` in a type argument overridden as `AnyRef`
//!   / `Any` (`gbmisc_javaobj*`).
//! * Auto-tupling ahead of argument prototypes, and no view into a tuple
//!   from a class that merely has the tuple's arity (`gbmisc_tupling`,
//!   `gbmisc_tuple_view_bad`).
//! * By-name implicit views (`gbmisc_byname_view`).
//! * Binary (scalac-compiled) parents: a case class's static `apply`
//!   forwarder, and a pickled abstract member under `super`
//!   (`gbmisc_binlib*`).
//!
//! Every fixture is also compiled by scalac 2.13.16, which must agree.
//! Fixture prefix: `gbmisc_`.

use std::collections::BTreeMap;
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
        "scala-rs-gbmisc-{tag}-{}-{nanos}-{seq}",
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

/// Compile `src` into `out` with `flags` and an optional extra class path.
fn compile(c: Compiler, src: &Path, out: &Path, flags: &[&str], cp: Option<&Path>) -> Output {
    let jar = scala_library_jar().unwrap();
    match c {
        Compiler::Scalac(sc) => {
            let mut classpath = jar.display().to_string();
            if let Some(cp) = cp {
                classpath = format!("{classpath}:{}", cp.display());
            }
            Command::new(sc)
                .args(["-classpath", &classpath, "-d", out.to_str().unwrap()])
                .args(flags)
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
            cmd.args(flags).output().expect("run scala-rs compile")
        }
    }
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
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
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

/// `name` compiles with `c` and prints the expected output.
fn check_runs(name: &str, c: Compiler, flags: &[&str], cp: Option<&Path>) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let o = compile(c, &src, &out, flags, cp);
    assert!(o.status.success(), "compile failed:\n{}", text(&o));
    assert_eq!(run_java(&out, cp), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// `name` is rejected by `c`, and the output mentions every one of `needles`.
fn check_rejects(name: &str, c: Compiler, flags: &[&str], cp: Option<&Path>, needles: &[&str]) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let o = compile(c, &src, &out, flags, cp);
    let t = text(&o);
    assert!(!o.status.success(), "{name} must not compile:\n{t}");
    for n in needles {
        assert!(t.contains(n), "{name}: expected `{n}` in:\n{t}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The (line, headline) pairs of every error, for scalac's and scala-rs's
/// formats alike.
fn error_lines(t: &str, file: &str) -> BTreeMap<(u32, String), usize> {
    let mut out = BTreeMap::new();
    let lines: Vec<&str> = t.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        // scalac: `path/file.scala:12: error: message`
        if let Some(rest) = l.split_once(&format!("{file}:")).map(|(_, r)| r) {
            if let Some((num, msg)) = rest.split_once(": error: ") {
                if let Ok(n) = num.parse::<u32>() {
                    *out.entry((n, msg.trim().to_string())).or_default() += 1;
                }
            }
            continue;
        }
        // scala-rs: `error: message`, its continuation lines, then
        // `  --> path/file.scala:12:5`
        if let Some(msg) = l.strip_prefix("error: ") {
            let at = lines[i + 1..]
                .iter()
                .take(6)
                .find(|x| x.trim_start().starts_with("-->"))
                .copied()
                .unwrap_or("");
            if let Some((_, pos)) = at.split_once(&format!("{file}:")) {
                if let Some(n) = pos.split(':').next().and_then(|n| n.parse::<u32>().ok()) {
                    *out.entry((n, msg.trim().to_string())).or_default() += 1;
                }
            }
        }
    }
    out
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

// ---------------------------------------------------------------- eta rule

#[test]
fn gbmisc_eta3_expands_unapplied_methods_under_xsource3() {
    with_jar(|| check_runs("gbmisc_eta3", Compiler::ScalaRs, &["-Xsource:3"], None));
    with_jar(|| {
        check_runs(
            "gbmisc_eta3",
            Compiler::ScalaRs,
            &["-Xsource:3-cross"],
            None,
        )
    });
}

#[test]
fn scalac_agrees_gbmisc_eta3() {
    with_scalac(|sc| check_runs("gbmisc_eta3", Compiler::Scalac(sc), &["-Xsource:3"], None));
}

/// Every error line and message of scala-rs is scalac's, and vice versa.
#[test]
fn gbmisc_eta2_bad_reports_exactly_scalacs_missing_argument_lists() {
    with_scalac(|sc| {
        let name = "gbmisc_eta2_bad";
        let file = format!("{name}.scala");
        let src = fixtures_dir().join(&file);
        let dir = tmp_dir(name);
        let (o1, o2) = (dir.join("s"), dir.join("r"));
        fs::create_dir_all(&o1).unwrap();
        fs::create_dir_all(&o2).unwrap();
        let s = compile(Compiler::Scalac(sc), &src, &o1, &[], None);
        let r = compile(Compiler::ScalaRs, &src, &o2, &[], None);
        assert!(!s.status.success() && !r.status.success());
        let se = error_lines(&text(&s), &file);
        let re = error_lines(&text(&r), &file);
        assert_eq!(se.values().sum::<usize>(), 25, "scalac:\n{}", text(&s));
        assert_eq!(re, se, "scala-rs:\n{}", text(&r));
        // The advice lines are part of the diagnostic.
        let t = text(&r);
        assert!(t.contains(
            "Unapplied methods are only converted to functions when a function type is expected."
        ));
        assert!(t.contains("You can make this conversion explicit by writing `add _` or `add(_,_)` instead of `add`."));
        assert!(t.contains("`curried _` or `curried(_)(_)` instead of `curried`"));
        assert!(t.contains("missing argument list for method local\n"));
        let _ = fs::remove_dir_all(&dir);
    });
}

#[test]
fn gbmisc_eta2_bad_is_accepted_under_xsource3_by_both() {
    with_scalac(|sc| {
        let name = "gbmisc_eta2_bad";
        let src = fixtures_dir().join(format!("{name}.scala"));
        let dir = tmp_dir(name);
        let (o1, o2) = (dir.join("s"), dir.join("r"));
        fs::create_dir_all(&o1).unwrap();
        fs::create_dir_all(&o2).unwrap();
        // `two` needs an implicit `Int` once it is eta-expanded; the rest
        // compiles under -Xsource:3.
        let s = compile(Compiler::Scalac(sc), &src, &o1, &["-Xsource:3"], None);
        let r = compile(Compiler::ScalaRs, &src, &o2, &["-Xsource:3"], None);
        let (st, rt) = (text(&s), text(&r));
        assert!(!s.status.success() && st.contains(":29: error: could not find implicit value"));
        assert!(!r.status.success(), "{rt}");
        assert_eq!(
            rt.matches("\nerror: ").count() + rt.starts_with("error: ") as usize,
            1,
            "{rt}"
        );
        assert!(rt.contains("gbmisc_eta2_bad.scala:29:"), "{rt}");
        let _ = fs::remove_dir_all(&dir);
    });
}

#[test]
fn gbmisc_eta_receiver_is_evaluated_once() {
    with_jar(|| check_runs("gbmisc_eta_receiver", Compiler::ScalaRs, &[], None));
}

#[test]
fn scalac_agrees_gbmisc_eta_receiver() {
    with_scalac(|sc| check_runs("gbmisc_eta_receiver", Compiler::Scalac(sc), &[], None));
}

// ------------------------------------------------- method reference arguments

#[test]
fn gbmisc_fnref_overloaded_method_as_function_argument() {
    with_jar(|| check_runs("gbmisc_fnref", Compiler::ScalaRs, &[], None));
}

#[test]
fn scalac_agrees_gbmisc_fnref() {
    with_scalac(|sc| check_runs("gbmisc_fnref", Compiler::Scalac(sc), &[], None));
}

#[test]
fn gbmisc_fnref_bad_rejected_by_both() {
    with_jar(|| {
        check_rejects(
            "gbmisc_fnref_bad",
            Compiler::ScalaRs,
            &[],
            None,
            &["gbmisc_fnref_bad.scala:11:", "gbmisc_fnref_bad.scala:12:"],
        )
    });
    with_scalac(|sc| {
        check_rejects(
            "gbmisc_fnref_bad",
            Compiler::Scalac(sc),
            &[],
            None,
            &[
                "gbmisc_fnref_bad.scala:11: error",
                "gbmisc_fnref_bad.scala:12: error",
            ],
        )
    });
}

// ------------------------------------------------------- named-argument apply

#[test]
fn gbmisc_named_apply_selects_overload_by_names() {
    with_jar(|| check_runs("gbmisc_named_apply", Compiler::ScalaRs, &[], None));
}

#[test]
fn scalac_agrees_gbmisc_named_apply() {
    with_scalac(|sc| check_runs("gbmisc_named_apply", Compiler::Scalac(sc), &[], None));
}

#[test]
fn gbmisc_named_apply_bad_rejected_by_both() {
    with_jar(|| {
        check_rejects(
            "gbmisc_named_apply_bad",
            Compiler::ScalaRs,
            &[],
            None,
            &[
                "gbmisc_named_apply_bad.scala:12:",
                "gbmisc_named_apply_bad.scala:13:",
            ],
        )
    });
    with_scalac(|sc| {
        check_rejects(
            "gbmisc_named_apply_bad",
            Compiler::Scalac(sc),
            &[],
            None,
            &[
                "gbmisc_named_apply_bad.scala:12: error",
                "gbmisc_named_apply_bad.scala:13: error",
            ],
        )
    });
}

// ------------------------------------------------ Java `Object` in overrides

/// javac-compiled `GbmiscMig` into a fresh directory, or `None` without javac.
fn java_mig() -> Option<PathBuf> {
    let dir = tmp_dir("javamig");
    let o = Command::new("javac")
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("gbmisc_java/GbmiscMig.java"))
        .output()
        .ok()?;
    o.status.success().then_some(dir)
}

#[test]
fn gbmisc_javaobj_override_with_anyref_type_argument() {
    with_jar(|| {
        let Some(cp) = java_mig() else {
            eprintln!("skip: javac not available");
            return;
        };
        check_runs("gbmisc_javaobj", Compiler::ScalaRs, &[], Some(&cp));
        check_rejects(
            "gbmisc_javaobj_bad",
            Compiler::ScalaRs,
            &[],
            Some(&cp),
            &[
                "method migrate overrides nothing",
                "object creation impossible",
            ],
        );
        let t = {
            let dir = tmp_dir("javaobj-bad-count");
            let o = compile(
                Compiler::ScalaRs,
                &fixtures_dir().join("gbmisc_javaobj_bad.scala"),
                &dir,
                &[],
                Some(&cp),
            );
            text(&o)
        };
        assert_eq!(t.matches("overrides nothing").count(), 2, "{t}");
        assert_eq!(t.matches("object creation impossible").count(), 2, "{t}");
    });
}

#[test]
fn scalac_agrees_gbmisc_javaobj() {
    with_scalac(|sc| {
        let Some(cp) = java_mig() else {
            eprintln!("skip: javac not available");
            return;
        };
        check_runs("gbmisc_javaobj", Compiler::Scalac(sc), &[], Some(&cp));
        check_rejects(
            "gbmisc_javaobj_bad",
            Compiler::Scalac(sc),
            &[],
            Some(&cp),
            &[
                "method migrate overrides nothing",
                "object creation impossible",
            ],
        );
    });
}

// ------------------------------------------------------ tupling and tuple views

#[test]
fn gbmisc_tupling_before_argument_prototypes() {
    with_jar(|| check_runs("gbmisc_tupling", Compiler::ScalaRs, &[], None));
}

#[test]
fn scalac_agrees_gbmisc_tupling() {
    with_scalac(|sc| check_runs("gbmisc_tupling", Compiler::Scalac(sc), &[], None));
}

#[test]
fn gbmisc_tuple_view_bad_rejected_by_both() {
    let needles = [
        "gbmisc_tuple_view_bad.scala:13:",
        "gbmisc_tuple_view_bad.scala:14:",
    ];
    with_jar(|| {
        check_rejects(
            "gbmisc_tuple_view_bad",
            Compiler::ScalaRs,
            &[],
            None,
            &needles,
        )
    });
    with_scalac(|sc| {
        check_rejects(
            "gbmisc_tuple_view_bad",
            Compiler::Scalac(sc),
            &[],
            None,
            &needles,
        )
    });
}

// ----------------------------------------------------------- by-name views

#[test]
fn gbmisc_byname_view_takes_the_argument_unevaluated() {
    with_jar(|| check_runs("gbmisc_byname_view", Compiler::ScalaRs, &[], None));
}

#[test]
fn scalac_agrees_gbmisc_byname_view() {
    with_scalac(|sc| check_runs("gbmisc_byname_view", Compiler::Scalac(sc), &[], None));
}

// ------------------------------------------------------ binary library parents

#[test]
fn gbmisc_binlib_static_forwarder_and_pickled_deferred_super() {
    with_scalac(|sc| {
        let lib = tmp_dir("binlib");
        let o = compile(
            Compiler::Scalac(sc),
            &fixtures_dir().join("gbmisc_binlib/GbmiscLib.scala"),
            &lib,
            &[],
            None,
        );
        assert!(o.status.success(), "{}", text(&o));
        check_runs("gbmisc_binlib_use", Compiler::ScalaRs, &[], Some(&lib));
        check_runs("gbmisc_binlib_use", Compiler::Scalac(sc), &[], Some(&lib));
        let _ = fs::remove_dir_all(&lib);
    });
}
