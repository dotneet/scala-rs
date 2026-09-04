//! E2E tests for the `agent/testkit2` slice: what a *user* of a compiled
//! Scala API needs from the classfile reader.
//!
//! `slick-testkit` reaches slick only through its class files, and every suite
//! in it is written in one shape:
//!
//! ```text
//! import tdb.profile.api._
//! class As(tag: Tag) extends Table[Int](tag, "a") {
//!   def id = column[Int]("id", O.PrimaryKey)
//! }
//! ```
//!
//! Four things in that shape were missing, all of them in the reader, all of
//! them found by pointing scala-rs at slick's **published** jar -- class files
//! real scalac wrote, so anything scala-rs reports there is scala-rs's:
//!
//! * **A nested class had no constructor.** nsc writes the `ScalaSignature` of
//!   a top-level class once, on the top-level class file; a nested class's own
//!   file carries a zero-length `Scala` marker and nothing else. So a class
//!   reached through a type alias -- which is how a slick profile exports
//!   `Table`, `Query`, `Sequence` -- was completed out of the *enclosing*
//!   pickle, where `PickleSupply::adopt_binary_class` skips `<init>` by name.
//!   `extends Table[Int](tag, "a")` was "no matching overload for constructor
//!   Table", and with the parent in error the body inherited nothing.
//!
//! * **A nullary type alias never reached scope.** `expose_unqualified_type`
//!   and `expose_from_wildcards` only entered a `Type::TypeMember`, and a
//!   nullary alias deliberately has no symbol -- it *is* its right-hand side.
//!   `type Tag = lifted.Tag`, and thirty more like it in `slick.lifted.
//!   Aliases`, therefore stayed unresolved names: "type mismatch; found: Tag
//!   required: Tag" for a parameter and its own constructor.
//!
//! * **`p.x.type` had no reading.** `val O: self.columnOptions.type` is how
//!   `RelationalTableComponent#Table` declares `O`. `conv` handled only a
//!   *module's* singleton type, so the whole member was declined and `O` kept
//!   the class file's erased accessor: "value PrimaryKey is not a member of
//!   RelationalTableComponent".
//!
//! * **An inherited member of a `-cp` class was never completed.**
//!   `complete_on_ancestors` walked only `scala.*` ancestors, and
//!   `enter_inherited_members` snapshots member lists that are still empty for
//!   a jar class. A selection reached nothing (`t.describe`) and a bare name
//!   inside the subclass body reached nothing either (`column`, 514
//!   diagnostics in one measurement).
//!
//! The fixture library is compiled by **real scalac** in the test that matters:
//! this is a test of the reader, and scalac's class files are the ones the
//! reader has to handle. scala-rs's own writer still loses two of the four
//! things above (see `docs/slick-testkit.md`), which is a separate defect and
//! not what these tests are about.
//!
//! Kept out of `crates/cli/tests/e2e.rs` and `testkit.rs` to avoid merge
//! conflicts with other agents; see `.agent-brief.md`. All fixtures use the
//! `testkit2` prefix.

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

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!("scala-rs-{tag}-{nanos}-{}", std::process::id()));
    fs::create_dir_all(&p).expect("create temp dir");
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
        .unwrap_or_else(|e| panic!("read expected/{name}.txt: {e}"))
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.exists().then_some(p)
}

fn find_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.exists().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_java(out: &Path, main: &str, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile `testkit2_lib.scala` with real scalac and return the output dir.
fn scalac_lib(scalac: &Path, tag: &str) -> PathBuf {
    let out = tmp_dir(tag);
    let status = Command::new(scalac)
        .args([
            fixtures_dir().join("testkit2_lib.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed on testkit2_lib");
    out
}

/// The recorded expectation is real scalac's, both halves compiled by it.
#[test]
fn real_scalac_dual_run_testkit2() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac testkit2: scalac or jar not obtainable");
        return;
    };
    let lib = scalac_lib(&scalac, "testkit2-sc-lib-ref");
    let usr = tmp_dir("testkit2-sc-use-ref");
    let status = Command::new(&scalac)
        .args([
            fixtures_dir().join("testkit2_use.scala").to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            usr.to_str().unwrap(),
        ])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed on testkit2_use");
    let cp = format!("{}:{}", lib.display(), jar.display());
    assert_eq!(
        run_java(&usr, "Main", &cp),
        expected_stdout("testkit2_use"),
        "recorded expectation for testkit2_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&usr);
    let _ = fs::remove_dir_all(&lib);
}

/// The test this slice is about: scala-rs compiles the *user* against class
/// files real scalac wrote, and the program has to behave identically.
///
/// Every one of the four gaps in the module comment shows up here. Before the
/// fixes this reported, in order: "no matching overload for constructor
/// Table", "type mismatch; found: Tag required: Tag", "value PrimaryKey is
/// not a member of Profile", "value describe is not a member of Users" and
/// "not found: value tableName".
#[test]
fn scala_rs_reads_scalac_classfiles_testkit2() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip testkit2 reader test: scalac or jar not obtainable");
        return;
    };
    let lib = scalac_lib(&scalac, "testkit2-sc-lib");
    let usr = tmp_dir("testkit2-rs-use");
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("testkit2_use.scala").to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            usr.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "scala-rs could not compile testkit2_use against scalac's classfiles: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let cp = format!("{}:{}", lib.display(), jar.display());
    assert_eq!(
        run_java(&usr, "Main", &cp),
        expected_stdout("testkit2_use"),
        "scala-rs's user of scalac's classfiles printed the wrong thing"
    );
    let _ = fs::remove_dir_all(&usr);
    let _ = fs::remove_dir_all(&lib);
}

/// Compile `testkit2_lib.scala` with **scala-rs** and return the output dir.
fn scala_rs_lib(jar: &Path, tag: &str) -> PathBuf {
    let out = tmp_dir(tag);
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("testkit2_lib.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "scala-rs failed on testkit2_lib: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

/// The same fixture the other way round: **scala-rs writes the library, real
/// scalac reads it**. Anything nsc reports here is this compiler's
/// `ScalaSignature` writer, which is what
/// `docs/slick-testkit.md` left as the acceptance test for the next slice.
///
/// Before nested classes and objects were declared in their owner's pickle
/// this reported ten errors, the first four of them: "value api is not a
/// member of object Profile", "not found: type Table", "not found: type Tag"
/// and "no arguments allowed for nullary constructor Object" -- `object
/// Profile { object api }` lost `api`, so both aliases it exports went with
/// it, and `Table`'s constructors were unreachable behind them.
#[test]
fn real_scalac_reads_scala_rs_classfiles_testkit2() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip testkit2 writer test: scalac or jar not obtainable");
        return;
    };
    let lib = scala_rs_lib(&jar, "testkit2-rs-lib");
    let usr = tmp_dir("testkit2-sc-use");
    let output = Command::new(&scalac)
        .args([
            fixtures_dir().join("testkit2_use.scala").to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            usr.to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        output.status.success(),
        "real scalac could not read scala-rs's classfiles: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let cp = format!("{}:{}", lib.display(), jar.display());
    assert_eq!(
        run_java(&usr, "Main", &cp),
        expected_stdout("testkit2_use"),
        "scalac-compiled user of scala-rs's classfiles printed the wrong thing"
    );
    let _ = fs::remove_dir_all(&usr);
    let _ = fs::remove_dir_all(&lib);
}
