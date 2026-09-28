//! The typed trees a def macro receives have the shape nsc's typer gives
//! them, which `showCode` -- and any macro that reads a tree's shape, such as
//! ScalaTest's `assert` or sourcecode's `Text` -- makes visible:
//!
//! * an empty-paren Java method used without its parentheses is applied
//!   (`s.length()`, not `s.length`);
//! * inferred type arguments are spelled out (`List.apply[Int](x, 3)`), and a
//!   function literal's parameters carry their types;
//! * a static path starts at its package (`scala.math.Numeric`, not
//!   `_root_.scala.math.Numeric`) and goes through the `scala` package object
//!   for what that object aliases (``scala.`package`.List``).
//!
//! The package at the head of a path still comes back resolved from the
//! root: a local named `scala` does not capture it. The macro library is
//! compiled by scalac; the client by scalac and by scala-rs, and both
//! programs print the same.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const REFLECT: &str = "/tmp/scala-2.13.16/lib/scala-reflect.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";

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
        "scala-rs-macro-tree-shape-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn compile(nsc: bool, source: &Path, cp: &str, out: &Path) {
    let mut cmd = Command::new(if nsc {
        NSC
    } else {
        env!("CARGO_BIN_EXE_scala-rs")
    });
    if nsc {
        cmd.env("JAVA_OPTS", "-Xmx2g -Xss8m");
    } else {
        cmd.args(["compile", "--scala-library", JAR]);
    }
    let r = cmd
        .arg(source)
        .args(["-cp", cp, "-d"])
        .arg(out)
        .output()
        .expect("run compiler");
    assert!(
        r.status.success(),
        "nsc={nsc}: {}{}",
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr)
    );
}

fn run(out: &Path) -> String {
    let o = Command::new("java")
        .args(["-cp", &format!("{}:{JAR}", out.display()), "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "Main failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn macro_arguments_have_nsc_typed_tree_shape() {
    if !Path::new(JAR).is_file() || !Path::new(NSC).is_file() {
        eprintln!("skip: scala-library or scalac not present");
        return;
    }
    let lib = tmp_dir("lib");
    compile(
        true,
        &fixtures_dir().join("macro_tree_shape_impl.scala"),
        REFLECT,
        &lib,
    );
    let cp = format!("{}:{REFLECT}", lib.display());
    let expected = fs::read_to_string(fixtures_dir().join("expected/macro_tree_shape.txt")).unwrap();
    for nsc in [true, false] {
        let out = tmp_dir(if nsc { "nsc" } else { "ours" });
        compile(
            nsc,
            &fixtures_dir().join("macro_tree_shape_use.scala"),
            &cp,
            &out,
        );
        assert_eq!(run(&out), expected, "nsc={nsc}");
        let _ = fs::remove_dir_all(&out);
    }
    let _ = fs::remove_dir_all(&lib);
}
