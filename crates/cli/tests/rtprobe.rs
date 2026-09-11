//! Silent miscompilations and wrong acceptances found by `tests/rt_probe.sh`.
//!
//! Every fixture here was reduced from a `tests/rtprobe/` program whose run
//! under scala-rs printed something other than scalac's run (or compiled a
//! program scalac rejects). None of them produced a diagnostic; each is pinned
//! by running the output and comparing it with the scalac-generated expected
//! file, and each expected file is itself checked against real scalac.
//!
//! Fixture prefix: `rtp_`.

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
        "scala-rs-rtprobe-{tag}-{}-{nanos}-{seq}",
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

fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
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

/// Compile fixture `name` with `compiler` (scala-rs when `None`), returning
/// the output directory and whether it compiled, with the compiler's output.
fn compile(name: &str, scalac_path: Option<&Path>, out: &Path) -> (bool, String) {
    let jar = scala_library_jar().expect("jar");
    let src = fixtures_dir().join(format!("{name}.scala"));
    let output = match scalac_path {
        Some(sc) => Command::new(sc)
            .args(["-nowarn", "-classpath", jar.to_str().unwrap(), "-d"])
            .arg(out)
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .arg("-d")
            .arg(out)
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    };
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}

/// Compile and run `name`, comparing with the expected output.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(name, scalac_path, &out);
    assert!(ok, "compile failed:\n{msgs}");
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// Both compilers reject `name`; scala-rs's diagnostics mention `needle`.
fn check_rejects(name: &str, needle: &str) {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(name, None, &out);
    assert!(
        !ok,
        "scala-rs accepted {name}, which scalac rejects:\n{msgs}"
    );
    assert!(msgs.contains(needle), "expected `{needle}` in:\n{msgs}");
    if let Some(sc) = scalac() {
        let sc_out = dir.join("sc");
        fs::create_dir_all(&sc_out).unwrap();
        let (sc_ok, sc_msgs) = compile(name, Some(&sc), &sc_out);
        assert!(
            !sc_ok,
            "scalac accepted {name} -- the fixture is wrong:\n{sc_msgs}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

macro_rules! runs {
    ($test:ident, $agree:ident, $name:literal) => {
        #[test]
        fn $test() {
            check_runs($name, None);
        }

        #[test]
        fn $agree() {
            let Some(sc) = scalac() else {
                eprintln!("skip: scalac not present");
                return;
            };
            check_runs($name, Some(&sc));
        }
    };
}

runs!(rtp_guards_runs, scalac_agrees_rtp_guards, "rtp_guards");
runs!(
    rtp_overload_bridge_runs,
    scalac_agrees_rtp_overload_bridge,
    "rtp_overload_bridge"
);
runs!(
    rtp_equality_runs,
    scalac_agrees_rtp_equality,
    "rtp_equality"
);
runs!(rtp_numeric_runs, scalac_agrees_rtp_numeric, "rtp_numeric");
runs!(
    rtp_eval_order_runs,
    scalac_agrees_rtp_eval_order,
    "rtp_eval_order"
);
runs!(
    rtp_valueclass_runs,
    scalac_agrees_rtp_valueclass,
    "rtp_valueclass"
);
runs!(rtp_sam_runs, scalac_agrees_rtp_sam, "rtp_sam");
runs!(rtp_arrays_runs, scalac_agrees_rtp_arrays, "rtp_arrays");
runs!(rtp_accepts_runs, scalac_agrees_rtp_accepts, "rtp_accepts");
runs!(
    rtp_null_unbox_runs,
    scalac_agrees_rtp_null_unbox,
    "rtp_null_unbox"
);
runs!(
    rtp_companion_early_runs,
    scalac_agrees_rtp_companion_early,
    "rtp_companion_early"
);
runs!(
    rtp_infer_lub_runs,
    scalac_agrees_rtp_infer_lub,
    "rtp_infer_lub"
);
runs!(
    rtp_widen_elem_runs,
    scalac_agrees_rtp_widen_elem,
    "rtp_widen_elem"
);

#[test]
fn rtp_rejects_map_of_non_pair() {
    check_rejects("rtp_neg_map_nonpair", "no matching overload");
}

#[test]
fn rtp_rejects_new_of_abstract_class() {
    check_rejects(
        "rtp_neg_abstract_new",
        "class A is abstract; cannot be instantiated",
    );
}

#[test]
fn rtp_rejects_new_of_trait() {
    check_rejects(
        "rtp_neg_trait_new",
        "trait T is abstract; cannot be instantiated",
    );
}

#[test]
fn rtp_rejects_final_class_parent() {
    check_rejects(
        "rtp_neg_final_class",
        "illegal inheritance from final class F",
    );
}

#[test]
fn rtp_rejects_case_to_case_inheritance() {
    check_rejects(
        "rtp_neg_case_case",
        "case-to-case inheritance is prohibited",
    );
}

#[test]
fn rtp_rejects_covariant_body_var() {
    check_rejects(
        "rtp_neg_variance_var",
        "covariant type A occurs in contravariant position",
    );
}

#[test]
fn rtp_rejects_anyval_type_test() {
    check_rejects(
        "rtp_neg_anyval_test",
        "type AnyVal cannot be used in a type pattern or isInstanceOf test",
    );
}

#[test]
fn rtp_rejects_local_var_default() {
    check_rejects(
        "rtp_neg_local_var_default",
        "local variables must be initialized",
    );
}

#[test]
fn rtp_rejects_local_self_reference() {
    check_rejects(
        "rtp_neg_local_self_ref",
        "forward reference extends over definition of value fibs",
    );
}

#[test]
fn rtp_rejects_subclass_overload_tie() {
    check_rejects("rtp_neg_overload_subclass", "ambiguous");
}

#[test]
fn rtp_rejects_sam_class_with_constructor_args() {
    check_rejects("rtp_neg_sam_class_args", "type mismatch");
}

#[test]
fn rtp_rejects_setter_defined_twice() {
    check_rejects("rtp_neg_var_setter", "method input_= is defined twice");
}
