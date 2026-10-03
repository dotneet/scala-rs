use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};

use crate::support::TestDir;

fn compile(child: &mut Child, source: &std::path::Path, output: &std::path::Path) -> u8 {
    let args = [
        source.to_string_lossy().into_owned(),
        "--no-scala-library".into(),
        "-d".into(),
        output.to_string_lossy().into_owned(),
    ];
    let stdin = child.stdin.as_mut().unwrap();
    for arg in args {
        stdin.write_all(arg.as_bytes()).unwrap();
        stdin.write_all(&[0]).unwrap();
    }
    stdin.write_all(&[0]).unwrap();
    stdin.flush().unwrap();
    let mut status = [0];
    child
        .stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut status)
        .unwrap();
    status[0]
}

#[test]
fn resident_compiler_keeps_runs_isolated_and_survives_a_failed_request() {
    let root = TestDir::new("resident-batch");
    let source = root.join("Reloaded.scala");
    let broken = root.join("Broken.scala");
    std::fs::write(&source, "object Reloaded { def value: Int = 1 }\n").unwrap();
    std::fs::write(&broken, "object Reloaded { def value: Int = }\n").unwrap();
    let outputs: Vec<_> = (0..4)
        .map(|index| {
            let path = root.join(format!("out-{index}"));
            std::fs::create_dir(&path).unwrap();
            path
        })
        .collect();
    let mut child = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .arg("__compile_batch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(compile(&mut child, &source, &outputs[0]), 0);
    std::fs::write(&source, "object Reloaded { def value: Int = 2 }\n").unwrap();
    assert_eq!(compile(&mut child, &source, &outputs[1]), 0);
    assert_eq!(compile(&mut child, &broken, &outputs[2]), 1);
    std::fs::write(&source, "object Reloaded { def value: Int = 1 }\n").unwrap();
    assert_eq!(compile(&mut child, &source, &outputs[3]), 0);
    drop(child.stdin.take());
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let first_class = std::fs::read(outputs[0].join("Reloaded$.class")).unwrap();
    let second_class = std::fs::read(outputs[1].join("Reloaded$.class")).unwrap();
    let again_class = std::fs::read(outputs[3].join("Reloaded$.class")).unwrap();
    assert_ne!(first_class, second_class);
    assert_eq!(first_class, again_class);
}

fn compile_with(child: &mut Child, args: &[String]) -> u8 {
    let stdin = child.stdin.as_mut().unwrap();
    for arg in args {
        stdin.write_all(arg.as_bytes()).unwrap();
        stdin.write_all(&[0]).unwrap();
    }
    stdin.write_all(&[0]).unwrap();
    stdin.flush().unwrap();
    let mut status = [0];
    child
        .stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut status)
        .unwrap();
    status[0]
}

fn engine_processes(parent: u32) -> Vec<u32> {
    let output = Command::new("pgrep")
        .args(["-P", &parent.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|pid| pid.parse().ok())
        .collect()
}

/// A resident compiler without the shared macro daemon keeps one engine JVM
/// for all its runs, whatever their classpaths, and the engine ends with it.
#[cfg(unix)]
#[test]
fn resident_compiler_keeps_one_macro_engine_without_the_daemon() {
    let library = std::path::PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    let reflect = std::path::PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    let java = Command::new("java").arg("-version").output();
    if !library.is_file() || !reflect.is_file() || java.is_err() {
        eprintln!("skip resident macro engine: scala-reflect or java not available");
        return;
    }
    let root = TestDir::new("resident-engine");
    let tmp = root.join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    let write = |name: &str, text: &str| {
        let path = root.join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let impl_source = write(
        "Twice.scala",
        "package m\nimport scala.language.experimental.macros\nimport scala.reflect.macros.blackbox\n\
         object Twice {\n  def apply(x: Int): Int = macro impl\n  \
         def impl(c: blackbox.Context)(x: c.Expr[Int]): c.Expr[Int] = {\n    \
         import c.universe._\n    c.Expr[Int](q\"$x + $x\")\n  }\n}\n",
    );
    let first_use = write("First.scala", "object First { val value: Int = m.Twice(1) }\n");
    let second_use = write(
        "Second.scala",
        "object Main { def main(args: Array[String]): Unit = println(First.value + m.Twice(20)) }\n",
    );
    let outputs: Vec<_> = ["impl", "first", "second"]
        .iter()
        .map(|name| {
            let path = root.join(name);
            std::fs::create_dir(&path).unwrap();
            path
        })
        .collect();
    let mut child = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .arg("__compile_batch")
        .env("SCALA_RS_MACRO_DAEMON", "0")
        .env("TMPDIR", &tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let request = |source: &std::path::Path, out: &std::path::Path, cp: &[&std::path::Path]| {
        let mut path = reflect.display().to_string();
        for entry in cp {
            path.push(':');
            path.push_str(&entry.display().to_string());
        }
        vec![
            source.display().to_string(),
            "-cp".into(),
            path,
            "-d".into(),
            out.display().to_string(),
            "--scala-library".into(),
            library.display().to_string(),
        ]
    };
    assert_eq!(
        compile_with(&mut child, &request(&impl_source, &outputs[0], &[])),
        0
    );
    assert_eq!(
        compile_with(&mut child, &request(&first_use, &outputs[1], &[&outputs[0]])),
        0
    );
    let engines = engine_processes(child.id());
    assert_eq!(engines.len(), 1, "one engine after the first expansion");
    assert_eq!(
        compile_with(
            &mut child,
            &request(&second_use, &outputs[2], &[&outputs[0], &outputs[1]])
        ),
        0
    );
    assert_eq!(engine_processes(child.id()), engines, "the same engine serves the next run");
    drop(child.stdin.take());
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let run = Command::new("java")
        .arg("-cp")
        .arg(format!(
            "{}:{}:{}",
            outputs[2].display(),
            outputs[1].display(),
            library.display()
        ))
        .arg("Main")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    // The engine reads its end of the pipe closing as the end of its work.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let alive = |pid: u32| Command::new("kill").args(["-0", &pid.to_string()]).output().unwrap().status.success();
    while alive(engines[0]) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!alive(engines[0]), "the engine outlived its compiler");
}
