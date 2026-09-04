//! Three erasure/bridge gaps found by walking `tests/slick_run.sh` forward
//! after the `-cp` value classes were fixed. Each one stopped the twelve
//! programs one line further on, and each is a rule scala-rs had approximated:
//!
//! 1. **The dominator of a compound type** (SLS 3.7, nsc's
//!    `intersectionDominator`) is not `parents.head`. slick's
//!    `q: Query[T, U, Seq] & TableQuery[T]` erases to `TableQuery`, because
//!    `TableQuery <: Query` shadows it, and a client compiled by real scalac
//!    calls `tableQueryToTableQueryExtensionMethods` at that descriptor --
//!    `NoSuchMethodError` on the first line of every program.
//! 2. **A bridge for an inherited member whose *parameter* was narrowed.**
//!    `emit_inherited_covariant_bridges` only bridged covariant *results*, so
//!    `H2Profile$` implemented `createSchemaActionExtensionMethods` at
//!    `SqlProfile#DDL` and nothing at the `SchemaDescription` the base
//!    interface declares: `AbstractMethodError` on `schema.create`.
//! 3. **A lambda parameter still typed as a tuple after erasure** had its
//!    `checkcast` hard-coded to `scala/Tuple2`. slick's
//!    `Resource[F, (Ref, CloseableIterator, …)].map(_._2)` cast to `Tuple2`
//!    and then called `Tuple3._2`, and the verifier threw the method out.
//!
//! The fixture is `tests/fixtures/erasure3.scala` and the expectation is real
//! scalac 2.13.16's own stdout for it, plus the two descriptors that a
//! separately compiled caller depends on.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/erasure3.scala")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-erasure3-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_main(cp: &str) -> String {
    let out = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        out.status.success(),
        "Main failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn compile_ours(out: &PathBuf, jar: &PathBuf) {
    let r = Command::new(bin())
        .args([
            "compile",
            fixture().to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        r.status.success(),
        "scala-rs failed on erasure3: {}",
        String::from_utf8_lossy(&r.stdout)
    );
}

fn javap(dir: &PathBuf, class: &str) -> Option<String> {
    let out = Command::new("javap")
        .args(["-p", "-cp", dir.to_str().unwrap(), class])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn erasure3_matches_scalac_stdout() {
    let (Some(scalac), Some(jar), true) = (find_scalac(), scala_library_jar(), java_available())
    else {
        eprintln!("skip erasure3: scalac, scala-library or java not obtainable");
        return;
    };
    let ref_out = tmp_dir("scalac");
    let status = Command::new(&scalac)
        .args(["-d", ref_out.to_str().unwrap(), fixture().to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(status.success(), "real scalac failed on erasure3");
    let expected = run_main(&format!("{}:{}", ref_out.display(), jar.display()));

    let ours = tmp_dir("rs");
    compile_ours(&ours, &jar);
    let actual = run_main(&format!("{}:{}", ours.display(), jar.display()));

    assert_eq!(actual, expected, "stdout differs from real scalac");
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&ours);
}

/// The descriptors themselves. stdout alone cannot see them: a separately
/// compiled caller is what links against them, and that is what slick's twelve
/// programs are.
#[test]
fn erasure3_descriptors_match_scalac() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip erasure3 javap: no scala-library");
        return;
    };
    let ours = tmp_dir("javap");
    compile_ours(&ours, &jar);
    let Some(dom) = javap(&ours, "Dominator$") else {
        eprintln!("skip erasure3 javap: javap unavailable");
        return;
    };
    // `Base with Derived` erases to `Derived`, `Marker with Base` to `Base`.
    assert!(
        dom.contains("shadowed(Derived)"),
        "compound-type dominator is not the unshadowed class:\n{dom}"
    );
    assert!(
        dom.contains("traitFirst(Base)"),
        "compound-type dominator took the trait over the class:\n{dom}"
    );
    let imp = javap(&ours, "Impl$").expect("javap Impl$");
    assert!(
        imp.contains("take(NarrowArg)") && imp.contains("take(Wide)"),
        "no parameter bridge for the narrowed inherited member:\n{imp}"
    );
    let _ = fs::remove_dir_all(&ours);
}
