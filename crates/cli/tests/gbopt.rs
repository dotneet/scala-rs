//! E2E tests for the `agent/gbopt` slice: three roots behind gitbucket's
//! slick implicit families, all three about what someone *else's* class files
//! say.
//!
//! 1. **The implicit scope's candidates were never warmed.**
//!    `warm_implicit_candidates` topped up the parents of every implicit in
//!    the *lexical* scope and of the wanted type, but not of the companion
//!    candidates `search_implicit_uncached` falls back on -- which are the
//!    ones a program is least likely to have named, since SLS 7.2 is exactly
//!    the rule that they need no import. slick's
//!    `TypedType.typedTypeToOptionTypedType[T]: OptionTypedType[T]` fits
//!    `TypedType[Option[String]]` only through `OptionTypedType`'s parent
//!    list, so every `column[Option[T]]` in gitbucket was "could not find
//!    implicit value of type TypedType[Option[String]]".
//!
//! 2. **A mixin forwarder flattened the parameter clauses.** A class carries
//!    a class-file method for every concrete method it inherits from a trait,
//!    with one flat parameter list -- and the class's own pickle does not
//!    declare it, so nothing replaced it and it shadowed the correctly
//!    clause-split declaration on the trait. slick's `inSet` / `inSetBind` /
//!    `in` reached through `BaseColumnExtensionMethods` are the case; `===`
//!    escaped only because its JVM name is `$eq$eq$eq`, which the class-file
//!    reader never decodes.
//!
//! 3. **A class file read once, but for the wrong owner.**
//!    `Checker::load_binary_into` reads each class file once and its
//!    short-circuit answered "already loaded" without checking that the owner
//!    now asking can see it. A JVM name cannot say whether `Outer$Inner$` was
//!    declared by the class or by the object, so the first route in decides,
//!    and `import scala.concurrent.ExecutionContext.Implicits.global` was
//!    "value Implicits is not a member of ExecutionContext$" in every file
//!    but the first.
//!
//! The library half is compiled by **real scalac** and handed over as class
//! files: none of the three can be shown from source alone.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn multi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/gbopt_binary")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-gbopt-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

/// Compile `Lib_1.scala` with real scalac into a fresh directory.
fn build_library(scalac: &Path, tag: &str) -> PathBuf {
    let out = tmp_dir(tag);
    let output = Command::new(scalac)
        .arg("-d")
        .arg(&out)
        .arg(multi_dir().join("Lib_1.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac Lib_1.scala failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn run_java(out: &Path, cp_extra: &str, main: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
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

/// The lines an error report blames, as `(line, message)`.
fn error_lines(text: &str) -> Vec<(u32, String)> {
    let mut errs = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("error: ") {
            pending = Some(rest.trim().to_string());
            continue;
        }
        if let (Some(msg), Some(pos)) = (pending.clone(), line.trim_start().strip_prefix("--> ")) {
            let mut parts = pos.rsplitn(3, ':');
            let _col = parts.next();
            if let Some(l) = parts.next().and_then(|n| n.parse::<u32>().ok()) {
                errs.push((l, msg));
            }
            pending = None;
        }
    }
    errs
}

/// All three roots at once: the program compiles against scalac's class files
/// and prints what scalac's own build of it prints.
///
/// On the pre-fix binary this reports, in order, "value Implicits is not a
/// member of ExecutionContext$", two "could not find implicit value of type
/// Box[Option[…]]" and two "no matching overload for (Iterable[Int],
/// Wit[Int, Int, R])Res[R] with arguments (List[Int])" -- one per root.
#[test]
fn multi_gbopt_binary_runs() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip gbopt_binary: scala-library jar or scalac not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let dir = multi_dir();
    let out = build_library(&scalac, "ok");
    let output = Command::new(bin())
        .args(["compile"])
        // The order matters: `Uses_1.scala` is what makes
        // `scala.concurrent.ExecutionContext` a stub before `Main_1.scala`'s
        // import walks it. See root 3.
        .arg(dir.join("Uses_1.scala"))
        .arg(dir.join("Main_1.scala"))
        .arg("-cp")
        .arg(&out)
        .arg("-d")
        .arg(&out)
        .args(["--scala-library", jar_s])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compiling against scalac's class files failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar_s, "Main"),
        fs::read_to_string(dir.join("expected.txt")).unwrap()
    );
    let _ = fs::remove_dir_all(&out);
}

/// The four programs real scalac 2.13.16 rejects, at its own lines.
///
/// Line 18 is the one the fix could have got wrong in the other direction:
/// splitting the forwarder's parameter list has to make the witness stop
/// being a positional argument, not merely make one call resolve. With the
/// same library packaged as a **jar**, the pre-fix binary compiled that line
/// -- one clause of two took the witness positionally, and scalac 2.13.16
/// says "too many arguments (found 2, expected 1)".
#[test]
fn multi_gbopt_binary_rejects_what_scalac_rejects() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip gbopt_binary bad: scala-library jar or scalac not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let dir = multi_dir();
    let out = build_library(&scalac, "bad");
    let bad_out = tmp_dir("bad-out");
    let output = Command::new(bin())
        .args(["compile"])
        .arg(dir.join("Bad_1.scala"))
        .arg("-cp")
        .arg(&out)
        .arg("-d")
        .arg(&bad_out)
        .args(["--scala-library", jar_s])
        .output()
        .expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "expected Bad_1.scala to fail");
    let lines: Vec<u32> = error_lines(&text).into_iter().map(|(l, _)| l).collect();
    for want in [16u32, 17, 18, 19] {
        assert!(
            lines.contains(&want),
            "no diagnostic on line {want} (scalac 2.13.16 reports one there): {text}"
        );
    }
    assert!(
        text.contains("could not find implicit value of type Box[Option[Int]]"),
        "line 16 should be the missing lift, got: {text}"
    );
    assert!(
        text.contains("with arguments (List[Int], Wit[Int, Int, Boolean])"),
        "line 18 should reject the witness as a positional argument, got: {text}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&bad_out);
}
