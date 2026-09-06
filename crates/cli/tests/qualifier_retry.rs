//! Cross-unit inference must revisit provisional qualifier errors, while
//! preserving genuine argument errors at their originating call.
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn inferred_parent_argument_qualifier_is_retried() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-qualifier-retry-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let scalac = "/tmp/scala-2.13.16/bin/scalac";
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    assert!(
        std::path::Path::new(scalac).is_file() && std::path::Path::new(jar).is_file(),
        "requires Scala 2.13.16"
    );
    let source = root.join("Source.scala");
    let helper = root.join("Helper.scala");
    fs::write(
        &source,
        r#"class Box(val statements: Vector[String])
class Artifacts { def sql: String = "ok" }
class Compiled {
  lazy val compiler = Helper.make
  lazy val upsert = compile(compiler)
  def compile(n: Int): Artifacts = new Artifacts
}
class Composer(val compiled: Compiled) {
  class Action extends Box(Vector(compiled.upsert.sql))
  def run = new Action().statements.head
}
object Main { def main(args: Array[String]): Unit = println(new Composer(new Compiled).run) }
"#,
    )
    .unwrap();
    for valid in [true, false] {
        fs::write(
            &helper,
            if valid {
                "object Helper { def make: Int = 1 }"
            } else {
                "object Helper { def make: String = \"bad\" }"
            },
        )
        .unwrap();
        for reverse in [false, true] {
            for ours in [false, true] {
                let out = root.join(format!("{valid}-{reverse}-{ours}"));
                fs::create_dir_all(&out).unwrap();
                let mut cmd = if ours {
                    let mut cmd = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                    cmd.args(["compile", "--scala-library", jar]);
                    cmd
                } else {
                    Command::new(scalac)
                };
                let files = if reverse {
                    [&helper, &source]
                } else {
                    [&source, &helper]
                };
                let result = cmd.args(files).arg("-d").arg(&out).output().unwrap();
                let stderr = String::from_utf8_lossy(&result.stderr);
                assert_eq!(
                    result.status.success(),
                    valid,
                    "valid={valid} reverse={reverse} ours={ours}: {stderr}"
                );
                if valid {
                    let cp = format!("{}:{jar}", out.display());
                    let result = Command::new("java")
                        .args(["-Xverify:all", "-cp", &cp, "Main"])
                        .output()
                        .unwrap();
                    assert!(
                        result.status.success(),
                        "{}",
                        String::from_utf8_lossy(&result.stderr)
                    );
                    assert_eq!(String::from_utf8_lossy(&result.stdout), "ok\n");
                } else {
                    assert!(stderr.contains("Source.scala:5:"), "{stderr}");
                    assert!(
                        stderr.contains("String") && stderr.contains("Int"),
                        "{stderr}"
                    );
                    assert!(!stderr.contains("select sql"), "{stderr}");
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn erroneous_application_chain_does_not_repeat_dynamic_receiver_typing() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let root = std::env::temp_dir().join(format!(
        "scala-rs-qualifier-growth-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("Main.scala");
    fs::write(
        &source,
        format!("object Main {{ val x = missing{} }}\n", ".f(0)".repeat(80)),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            "--scala-library",
            "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
        ])
        .arg(&source)
        .arg("-d")
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The repaired release compiler takes less than a second here. The old
    // repeated traversal already exceeded five seconds at depth 16, so 80
    // makes this a generous termination check rather than a microbenchmark.
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "erroneous qualifier chain did not finish within 20 seconds; source: {}",
                source.display()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let result = child.wait_with_output().unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("not found: value missing"), "{stderr}");
    assert_eq!(stderr.matches("error:").count(), 1, "{stderr}");
    fs::remove_dir_all(root).unwrap();
}
