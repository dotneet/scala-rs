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
