//! Higher-kinded lower bounds retain their constructor through inference.
//! The real fs2 case checks `Stream.compile` / `evalMap`; the source-only case
//! pins the inferred public JVM descriptor against scalac 2.13.16.

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

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-fs2-io-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn scala_library() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.is_file().then_some(path)
}

fn scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|out| out.status.success() || !out.stdout.is_empty() || !out.stderr.is_empty())
        .unwrap_or(false)
}

fn diagnostics(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

fn fs2_classpath() -> Option<String> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let roots = [
        home.join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        home.join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ];
    let jars = [
        (
            "org/typelevel/cats-core_2.13/2.13.0",
            "cats-core_2.13-2.13.0.jar",
        ),
        (
            "org/typelevel/cats-kernel_2.13/2.13.0",
            "cats-kernel_2.13-2.13.0.jar",
        ),
        (
            "org/typelevel/cats-effect_2.13/3.7.1",
            "cats-effect_2.13-3.7.1.jar",
        ),
        (
            "org/typelevel/cats-effect-kernel_2.13/3.7.1",
            "cats-effect-kernel_2.13-3.7.1.jar",
        ),
        (
            "org/typelevel/cats-effect-std_2.13/3.7.1",
            "cats-effect-std_2.13-3.7.1.jar",
        ),
        ("co/fs2/fs2-core_2.13/3.13.0", "fs2-core_2.13-3.13.0.jar"),
    ];
    let paths: Option<Vec<PathBuf>> = jars
        .into_iter()
        .map(|(dir, file)| {
            roots
                .iter()
                .map(|root| root.join(dir).join(file))
                .find(|path| path.is_file())
        })
        .collect();
    Some(
        std::env::join_paths(paths?)
            .ok()?
            .to_string_lossy()
            .into_owned(),
    )
}

fn javap(out: &Path, class: &str) -> String {
    let result = Command::new("javap")
        .args(["-p", "-s", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("run javap");
    assert!(
        result.status.success(),
        "javap failed: {}",
        diagnostics(&result)
    );
    String::from_utf8(result.stdout).expect("javap output is UTF-8")
}

fn method_descriptor(text: &str, name: &str) -> String {
    let marker = format!(" {name}(");
    let mut lines = text.lines().skip_while(|line| !line.contains(&marker));
    lines
        .next()
        .unwrap_or_else(|| panic!("method {name} not found in:\n{text}"));
    lines
        .find_map(|line| line.trim().strip_prefix("descriptor: "))
        .unwrap_or_else(|| panic!("descriptor for {name} not found in:\n{text}"))
        .to_owned()
}

#[test]
fn fs2_compile_ops_preserve_io_arity() {
    let Some(cp) = fs2_classpath() else {
        eprintln!("skip fs2 IO arity regression: Coursier dependencies unavailable");
        return;
    };
    let out = tmp_dir("compile");
    let result = Command::new(bin())
        .args([
            "compile",
            fixture("io_stream_arity").to_str().unwrap(),
            "-cp",
            &cp,
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "fs2 IO arity fixture failed: {}",
        diagnostics(&result)
    );
    let _ = fs::remove_dir_all(out);
}

#[test]
fn higher_kinded_lower_bound_matches_scalac_abi() {
    let (Some(library), Some(scalac), true) = (scala_library(), scalac(), tool_available("javap"))
    else {
        eprintln!("skip HK lower-bound ABI: scala-library, scalac or javap unavailable");
        return;
    };
    let mine = tmp_dir("abi-mine");
    let theirs = tmp_dir("abi-scalac");
    let source = fixture("io_hk_lower_abi");
    let ours = Command::new(bin())
        .arg("compile")
        .arg(&source)
        .args(["--scala-library", library.to_str().unwrap()])
        .args(["-d", mine.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        ours.status.success(),
        "scala-rs failed: {}",
        diagnostics(&ours)
    );
    let reference = Command::new(scalac)
        .arg(&source)
        .args(["-d", theirs.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        reference.status.success(),
        "scalac failed: {}",
        diagnostics(&reference)
    );
    let ours = method_descriptor(&javap(&mine, "IOHkLowerAbi$"), "inferred");
    let reference = method_descriptor(&javap(&theirs, "IOHkLowerAbi$"), "inferred");
    assert_eq!(ours, reference, "inferred ABI must match real scalac");
    assert_eq!(ours, "(LHC;)Lscala/collection/immutable/List;");
    let _ = fs::remove_dir_all(mine);
    let _ = fs::remove_dir_all(theirs);
}
