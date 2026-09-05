//! Runtime reflection through `currentMirror`, and `scala.tools.reflect
//! .ToolBox`. `docs/notes/macro-reflect-and-reify.md`.
//!
//! Everything here needs scala-reflect.jar on the classpath, and the toolbox
//! also needs scala-compiler.jar -- `mkToolBox` returns a real nsc instance,
//! and `eval` compiles and runs the tree at run time. Those two jars are why
//! these tests are in their own file rather than in `e2e.rs`, the same reason
//! `engine.rs` (`rd_*`, `rb_*`, `rt_*`) is.
//!
//! The check that matters is the **dual run**: real scalac 2.13.16 compiles
//! and runs the same fixtures, and the two programs must print the same
//! thing. A mirror or a toolbox that was built wrongly still compiles and
//! still runs; only the output tells them apart.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-toolbox-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn tool_available(what: &str) -> bool {
    Command::new(what)
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scala_reflect_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    p.is_file().then_some(p)
}

fn scala_compiler_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/lib/scala-compiler.jar");
    p.is_file().then_some(p)
}

fn find_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

/// scala-reflect.jar plus scala-compiler.jar, which the toolbox needs at both
/// compile time (`scala.tools.reflect.ToolBox` lives there) and run time.
fn reflect_cp() -> String {
    format!(
        "{}:{}",
        scala_reflect_jar().unwrap().display(),
        scala_compiler_jar().unwrap().display()
    )
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
        .unwrap_or_else(|e| panic!("read expected/{name}.txt: {e}"))
}

/// Everything these tests need. Returns false (and says so) when the machine
/// cannot run them at all.
fn prerequisites(tag: &str) -> bool {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("skip {tag}: java / javac not available");
        return false;
    }
    if scala_library_jar().is_none()
        || scala_reflect_jar().is_none()
        || scala_compiler_jar().is_none()
    {
        eprintln!("skip {tag}: scala-library / scala-reflect / scala-compiler not obtainable");
        return false;
    }
    true
}

/// Compile `<name>.scala` with scala-rs, with `extra` ahead of the two jars.
fn compile(name: &str, out: &Path, extra: &[&Path]) -> std::process::Output {
    let jar = scala_library_jar().expect("scala-library");
    let mut cp = String::new();
    for e in extra {
        cp.push_str(&e.display().to_string());
        cp.push(':');
    }
    cp.push_str(&reflect_cp());
    Command::new(bin())
        .args([
            "compile",
            fixture(name).to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            &cp,
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile")
}

/// Compile `<name>.scala` with real scalac, with `extra` ahead of the jars.
fn scalac(name: &str, out: &Path, extra: &[&Path]) -> std::process::Output {
    let mut cp = String::new();
    for e in extra {
        cp.push_str(&e.display().to_string());
        cp.push(':');
    }
    cp.push_str(&reflect_cp());
    Command::new(find_scalac().expect("scalac"))
        .args([
            "-cp",
            &cp,
            "-d",
            out.to_str().unwrap(),
            fixture(name).to_str().unwrap(),
        ])
        .output()
        .expect("run scalac")
}

/// Run `Main` out of `dirs` and return its stdout, asserting a clean exit.
fn run_main(dirs: &[&Path], what: &str) -> String {
    let mut cp = String::new();
    for d in dirs {
        cp.push_str(&d.display().to_string());
        cp.push(':');
    }
    cp.push_str(&scala_library_jar().unwrap().display().to_string());
    cp.push(':');
    cp.push_str(&reflect_cp());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for {what}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// `currentMirror`, the two `RuntimeClass`-taking mirror methods, the pickled
/// `Nothing` a reflected signature reads back, and the toolbox.
#[test]
fn tb_reflect_runs() {
    if !prerequisites("tb_reflect") {
        return;
    }
    let out_dir = tmp_dir("tb_reflect");
    let out = compile("tb_reflect", &out_dir, &[]);
    assert!(
        out.status.success(),
        "compile tb_reflect failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&[&out_dir], "tb_reflect"),
        expected_stdout("tb_reflect")
    );
}

/// The same fixture under real scalac 2.13.16. A mirror built against the
/// wrong class loader, or a toolbox handed the wrong universe, compiles and
/// runs -- only the output separates it from the right one.
#[test]
fn tb_reflect_matches_real_scalac() {
    if !prerequisites("tb_reflect") || find_scalac().is_none() {
        eprintln!("skip tb_reflect_matches_real_scalac: scalac not available");
        return;
    }
    let out_dir = tmp_dir("tb_reflect_scalac");
    let out = scalac("tb_reflect", &out_dir, &[]);
    assert!(
        out.status.success(),
        "scalac tb_reflect failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&[&out_dir], "tb_reflect (scalac)"),
        expected_stdout("tb_reflect")
    );
}

/// `import c.{prefix => prefix}` inside a macro implementation: a named import
/// of a member of a *value*, which only the `ScalaSignature` declares.
#[test]
fn tb_prefix_import_expands_and_runs() {
    if !prerequisites("tb_prefix_use") {
        return;
    }
    let impls = tmp_dir("tb_prefix_impl");
    let uses = tmp_dir("tb_prefix_use");
    let out = compile("tb_prefix_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "compile tb_prefix_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile("tb_prefix_use", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile tb_prefix_use failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&[&uses, &impls], "tb_prefix_use"),
        expected_stdout("tb_prefix_use")
    );
}

/// The same two files, both stages under real scalac.
#[test]
fn tb_prefix_import_matches_real_scalac() {
    if !prerequisites("tb_prefix_use") || find_scalac().is_none() {
        eprintln!("skip tb_prefix_import_matches_real_scalac: scalac not available");
        return;
    }
    let impls = tmp_dir("tb_prefix_impl_scalac");
    let uses = tmp_dir("tb_prefix_use_scalac");
    let out = scalac("tb_prefix_impl", &impls, &[]);
    assert!(
        out.status.success(),
        "scalac tb_prefix_impl failed: {}",
        diagnostics(&out)
    );
    let out = scalac("tb_prefix_use", &uses, &[&impls]);
    assert!(
        out.status.success(),
        "scalac tb_prefix_use failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&[&uses, &impls], "tb_prefix_use (scalac)"),
        expected_stdout("tb_prefix_use")
    );
}

/// What is still unimplemented is *named*, not silently accepted. Real scalac
/// compiles and runs `tb_bad.scala`; scala-rs reports one error per line and
/// emits nothing.
#[test]
fn tb_bad_is_named_not_stubbed() {
    if !prerequisites("tb_bad") {
        return;
    }
    let out_dir = tmp_dir("tb_bad");
    let out = compile("tb_bad", &out_dir, &[]);
    assert!(
        !out.status.success(),
        "tb_bad compiled, but scala-rs cannot reach `api.Mirror`'s members: {}",
        diagnostics(&out)
    );
    let text = diagnostics(&out);
    for name in ["staticClass", "staticModule", "staticPackage"] {
        assert!(
            text.contains(&format!("value {name} is not a member of")),
            "tb_bad should name {name} as unreachable, got: {text}"
        );
    }
}
