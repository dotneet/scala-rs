//! Parameter names in the `LocalVariableTable`, as scalac's default `-g:vars`
//! writes them. Libraries read them off the bytecode rather than out of
//! `MethodParameters`: jackson-module-scala's paranamer pairs a case class's
//! properties with its constructor this way, and without the table it lost
//! every annotation written on a constructor parameter.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::path::Path;
use std::{fs, process::Command};

const SOURCE: &str = r#"
package p
case class C(name: String, count: Long, flag: Boolean)
class D(val a: Int) { def m(x: Double, y: String): String = y * x.toInt }
object O { def s(xs: Array[Int], k: Int): Int = xs.length + k }
case class Q(`https://a.b/c`: String, `x y`: Int)
"#;

/// `(method, slot, name, descriptor)` of every table row whose slot is a
/// parameter's (or `this`'s), from `javap -l`.
fn param_rows(javap: &Path, class: &Path) -> Vec<String> {
    let text = String::from_utf8(
        Command::new(javap)
            .args(["-l", "-p"])
            .arg(class)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let mut out = Vec::new();
    let mut method = String::new();
    let mut params = 0usize;
    for line in text.lines() {
        let t = line.trim();
        if t.ends_with(");") {
            method = t.to_string();
            let inside = &t[t.find('(').unwrap() + 1..t.rfind(')').unwrap()];
            params = inside.split(',').filter(|s| !s.trim().is_empty()).count()
                + usize::from(!t.contains(" static "));
            continue;
        }
        let cols: Vec<&str> = t.split_whitespace().collect();
        if cols.len() == 5
            && cols[0] == "0"
            && cols[2].parse::<usize>().is_ok_and(|s| s < params + 2)
        {
            out.push(format!("{method} {} {} {}", cols[2], cols[3], cols[4]));
        }
    }
    out.sort();
    out
}

#[test]
fn parameters_are_named_in_the_local_variable_table() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let javap = java.with_file_name("javap");
    if !javap.is_file() {
        eprintln!("skip: javap is unavailable");
        return;
    }
    let dir = TestDir::new("param-local-names");
    let source = dir.join("C.scala");
    fs::write(&source, SOURCE).unwrap();
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    CompileCommand::new(&source, &ours)
        .classpath(library)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let output = Command::new(scalac)
        .arg("-d")
        .arg(&theirs)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for class in ["p/C.class", "p/D.class", "p/O$.class", "p/Q.class"] {
        let expected = param_rows(&javap, &theirs.join(class));
        let actual = param_rows(&javap, &ours.join(class));
        let wanted: Vec<&String> = expected
            .iter()
            .filter(|r| {
                r.contains("p.C(")
                    || r.contains(" m(")
                    || r.contains(" s(")
                    || r.contains("p.D(")
                    || r.contains("p.Q(")
            })
            .collect();
        assert!(!wanted.is_empty(), "{class}: scalac rows {expected:?}");
        for row in wanted {
            assert!(
                actual.contains(row),
                "{class}: missing {row}\nours: {actual:#?}"
            );
        }
    }
    // The JVM checks the names in the table: the class must load.
    let probe = dir.join("Probe.java");
    fs::write(
        &probe,
        "public class Probe { public static void main(String[] a) throws Exception { \
         System.out.println(Class.forName(\"p.Q\").getDeclaredConstructors().length); } }",
    )
    .unwrap();
    if let Some(javac) = t.javac() {
        let probe_out = dir.join("probe");
        fs::create_dir_all(&probe_out).unwrap();
        let c = Command::new(javac)
            .arg("-d")
            .arg(&probe_out)
            .arg(&probe)
            .output()
            .unwrap();
        assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
        let run = Command::new(java)
            .arg("-cp")
            .arg(format!(
                "{}:{}:{}",
                probe_out.display(),
                ours.display(),
                library.display()
            ))
            .arg("Probe")
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
}
