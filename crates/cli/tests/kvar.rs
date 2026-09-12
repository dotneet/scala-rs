//! Kind conformance of higher-kinded type arguments (nsc `checkKindBounds`)
//! and the variance of a type lambda's own parameters (nsc
//! `validateVariance`), both in `crates/typer/src/kind_bounds.rs`.
//!
//! `kvar_kind_ok.scala` runs and must print what scalac's build prints: every
//! valid higher-kinded shape it exercises must stay accepted.
//! `kvar_kind_bad.scala` (kind errors, typer) and `kvar_lambda_bad.scala`
//! (variance errors, refchecks) must be rejected on exactly the lines marked
//! `// error`, which are the lines scalac rejects. `kvar_t7872b.scala` and
//! `kvar_t7872c.scala` are the scala/scala corpus programs, with the exact
//! message text checked against scalac's. `kvar_kp_bad.scala` is the
//! kind-projector spelling (`-Ykind-projector`, scala-rs only).
//!
//! Fixture prefix: `kvar_`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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
        "scala-rs-kvar-{tag}-{}-{nanos}-{seq}",
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

fn compile(
    src: &Path,
    out: &Path,
    jar: &Path,
    scalac_path: Option<&Path>,
    extra: &[&str],
) -> std::process::Output {
    match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .args(extra)
            .output()
            .expect("run scala-rs compile"),
    }
}

fn output_text(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
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
    let output = compile(&src, &out, &jar, scalac_path, &[]);
    assert!(
        output.status.success(),
        "compile {name} failed:\n{}",
        output_text(&output)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        run.status.success(),
        "java Main failed:\n{}",
        output_text(&run)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// The lines of `name` that end in `// error`.
fn marked_lines(name: &str) -> BTreeSet<usize> {
    let src = fs::read_to_string(fixtures_dir().join(format!("{name}.scala"))).unwrap();
    src.lines()
        .enumerate()
        .filter(|(_, l)| l.trim_end().ends_with("// error"))
        .map(|(i, _)| i + 1)
        .collect()
}

/// Compile `name` (which must be rejected) and return the diagnostic text.
fn rejected_text(name: &str, scalac_path: Option<&Path>, extra: &[&str]) -> Option<String> {
    let jar = scala_library_jar()?;
    let src = fixtures_dir().join(format!("{name}.scala"));
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(&src, &out, &jar, scalac_path, extra);
    let text = output_text(&output);
    assert!(!output.status.success(), "{name} compiled:\n{text}");
    let _ = fs::remove_dir_all(&dir);
    Some(text)
}

/// The lines of `name` a compiler reported an error on.
fn error_lines(name: &str, scalac_path: Option<&Path>, extra: &[&str]) -> Option<BTreeSet<usize>> {
    let text = rejected_text(name, scalac_path, extra)?;
    let file = format!("{name}.scala:");
    let mut lines = BTreeSet::new();
    let mut after_error = scalac_path.is_some();
    for l in text.lines() {
        if scalac_path.is_none() {
            // scala-rs prints `error: …` and then `  --> path:line:col`.
            if l.starts_with("error") {
                after_error = true;
                continue;
            }
            if !after_error {
                continue;
            }
        } else if !l.contains(": error:") {
            continue;
        }
        if let Some(at) = l.find(&file) {
            let rest = &l[at + file.len()..];
            let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = n.parse() {
                lines.insert(n);
            }
            if scalac_path.is_none() {
                after_error = false;
            }
        }
    }
    Some(lines)
}

fn rejected_on_marked_lines(name: &str, scalac_path: Option<&Path>, extra: &[&str]) {
    let Some(got) = error_lines(name, scalac_path, extra) else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    assert_eq!(got, marked_lines(name), "{name}: error lines");
}

#[test]
fn kvar_kind_ok_runs() {
    check_runs("kvar_kind_ok", None);
}

#[test]
fn scalac_agrees_kvar_kind_ok() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("kvar_kind_ok", Some(&sc));
}

#[test]
fn kvar_kind_bad_is_rejected_on_the_marked_lines() {
    rejected_on_marked_lines("kvar_kind_bad", None, &[]);
}

#[test]
fn scalac_agrees_kvar_kind_bad() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    rejected_on_marked_lines("kvar_kind_bad", Some(&sc), &[]);
}

#[test]
fn kvar_classkind_bad_is_rejected_on_the_marked_lines() {
    rejected_on_marked_lines("kvar_classkind_bad", None, &[]);
}

#[test]
fn scalac_agrees_kvar_classkind_bad() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    rejected_on_marked_lines("kvar_classkind_bad", Some(&sc), &[]);
}

#[test]
fn kvar_lambda_bad_is_rejected_on_the_marked_lines() {
    rejected_on_marked_lines("kvar_lambda_bad", None, &[]);
}

#[test]
fn scalac_agrees_kvar_lambda_bad() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    rejected_on_marked_lines("kvar_lambda_bad", Some(&sc), &[]);
}

#[test]
fn kvar_kp_bad_is_rejected_on_the_marked_lines() {
    rejected_on_marked_lines("kvar_kp_bad", None, &["-Ykind-projector"]);
}

/// nsc's `KindBoundErrors` message for `neg/t7872c`, word for word.
const T7872C_MESSAGE: &str = "inferred kinds of the type arguments (List) do not conform to the expected kinds of the type parameters (type F).\nList's type parameters do not match type F's expected parameters:\ntype A is covariant, but type _ is declared contravariant";

/// nsc's variance message for the first lambda of `neg/t7872b`, word for word.
const T7872B_MESSAGE: &str =
    "contravariant type a occurs in covariant position in type [-a]List[a] of value <local l>";

/// The second lambda of `neg/t7872b`; the body is `Stringer[a]` dealiased,
/// and the two compilers print a function type with and without parentheses
/// around its parameter, so only the fixed parts are compared.
const T7872B_SECOND_HEAD: &str = "covariant type a occurs in contravariant position in type [+a]";
const T7872B_SECOND_TAIL: &str = " => String of value <local l>";

#[test]
fn t7872c_kind_message_is_nscs() {
    let Some(text) = rejected_text("kvar_t7872c", None, &[]) else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    assert!(text.contains(T7872C_MESSAGE), "scala-rs output:\n{text}");
    // The definitions `up` and `down` are correct: only the call is rejected.
    assert_eq!(
        error_lines("kvar_t7872c", None, &[]).unwrap(),
        BTreeSet::from([10])
    );
}

#[test]
fn scalac_agrees_t7872c_kind_message() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let Some(text) = rejected_text("kvar_t7872c", Some(&sc), &[]) else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    assert!(text.contains(T7872C_MESSAGE), "scalac output:\n{text}");
}

#[test]
fn t7872b_variance_messages_are_nscs() {
    let Some(text) = rejected_text("kvar_t7872b", None, &[]) else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    assert!(text.contains(T7872B_MESSAGE), "scala-rs output:\n{text}");
    assert!(
        text.lines().any(|l| {
            l.contains(T7872B_SECOND_HEAD) && l.trim_end().ends_with(T7872B_SECOND_TAIL)
        }),
        "scala-rs output:\n{text}"
    );
    assert_eq!(
        error_lines("kvar_t7872b", None, &[]).unwrap(),
        BTreeSet::from([11, 17])
    );
}

#[test]
fn scalac_agrees_t7872b_variance_messages() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let Some(text) = rejected_text("kvar_t7872b", Some(&sc), &[]) else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    assert!(text.contains(T7872B_MESSAGE), "scalac output:\n{text}");
    assert!(
        text.lines().any(|l| {
            l.contains(T7872B_SECOND_HEAD) && l.trim_end().ends_with(T7872B_SECOND_TAIL)
        }),
        "scalac output:\n{text}"
    );
    assert_eq!(
        error_lines("kvar_t7872b", Some(&sc), &[]).unwrap(),
        BTreeSet::from([11, 17])
    );
}
