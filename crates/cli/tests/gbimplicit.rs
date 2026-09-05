//! E2E tests for the `agent/gbimplicit` slice: the `Shape` half of
//! gitbucket's remaining slick implicit clusters.
//!
//! `tests/gitbucket_measure.sh` goes **`errors=981 → 947`**, and the
//! `could not find implicit value of type Shape[…]` cluster **45 → 21**.
//! slick (`errors=0 classes=1490`) is unchanged.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. The jar-backed test uses the published
//! `slick_2.13-3.4.1.jar` from the local Coursier cache and skips when it is
//! not there, the way `crates/cli/tests/slickimplicit.rs` does.
//!
//! **The root.** slick's second `Shape` witness is
//!
//! ```text
//! AbstractTable.tableShape[Level <: ShapeLevel, T, C <: AbstractTable[_]]
//!   (implicit ev: C <:< AbstractTable[T]): Shape[Level, C, T, C]
//! ```
//!
//! and it answers every `q.map(a => a)`, `q.map(a => (a, a.age))`, and every
//! join whose shape mentions a table. `Query.map[F, G, T](f: E => F)(implicit
//! shape: Shape[_ <: FlatShapeLevel, F, T, G]): Query[G, T, C]` leaves `T` and
//! `G` for the search to solve, so the wanted type is
//! `Shape[_ <: FlatShapeLevel, Accounts, ?T, ?G]`.
//!
//! Unifying the candidate's result with that settles `Level`, `C := Accounts`
//! and `?G := Accounts` -- and pairs the candidate's own `T` with the call
//! site's `?T`, neither of which is known. `implicit_solve` then fell back to
//! its one-sided guess, which answers `T := ?T`, and reported the candidate
//! *solved*. The clause check that follows therefore asked for
//! `Accounts <:< AbstractTable[?T]` with `?T` a free type parameter, which no
//! witness can match, and the whole family was "could not find implicit value
//! of type Shape[_ <: FlatShapeLevel, Accounts, T, G]".
//!
//! `Typer::implicit_fit_open` is exactly the fallback that settles a
//! candidate's own parameters from its own implicit clause -- `<:<.refl`
//! matches `Accounts <:< AbstractTable[?T]` by widening `Accounts` to its base
//! type at `AbstractTable`, which gives `T` -- but it only ran when
//! `implicit_solve` had *failed*. It now also runs when the solve succeeded on
//! paper but the clause search it implies could not have: when one of the
//! targs it returned still mentions a call-site parameter the search has to
//! solve. `Unify::alias_of` carries the answer back to that call-site
//! parameter, since unification bound the candidate's side to it and not the
//! other way round.
//!
//! **Not fixed** (`docs/gitbucket.md`, "not fixed: blocking-slick's
//! conversions under `import profile.blockingApi._`"): the ~170 `value list /
//! update / firstOption is not a member of Query[…]` diagnostics. The cause is
//! known and reproduced in fifteen lines; the fix is one guard and it makes
//! `tests/gitbucket_measure.sh` more than fifty times slower.

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
        "scala-rs-gbimplicit-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn real_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// slick 3.4.1 and the jars it needs, from the local Coursier cache if they
/// happen to be there. Nothing is downloaded. Same list as
/// `crates/cli/tests/slickimplicit.rs`.
fn slick_jars() -> Option<Vec<PathBuf>> {
    let home = std::env::var("HOME").ok()?;
    let roots = [
        PathBuf::from(&home).join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        PathBuf::from(&home).join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ];
    let wanted = [
        ("com/typesafe/slick/slick_2.13", "slick_2.13", Some("3.4.1")),
        ("com/typesafe/config", "config", None),
        ("org/slf4j/slf4j-api", "slf4j-api", None),
        (
            "org/reactivestreams/reactive-streams",
            "reactive-streams",
            None,
        ),
    ];
    let mut out = Vec::new();
    for (rel, prefix, pin) in wanted {
        let mut found = None;
        for root in &roots {
            let Ok(rd) = fs::read_dir(root.join(rel)) else {
                continue;
            };
            for ent in rd.flatten() {
                let version = ent.file_name().to_string_lossy().into_owned();
                if pin.is_some_and(|p| p != version) {
                    continue;
                }
                let candidate = ent.path().join(format!("{prefix}-{version}.jar"));
                if candidate.is_file() {
                    found = Some(candidate);
                }
            }
        }
        out.push(found?);
    }
    Some(out)
}

fn classpath(jars: &[PathBuf]) -> String {
    jars.iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// Compile one fixture. Answers (success, diagnostics, output directory).
fn compile(name: &str, extra: &[&str]) -> (bool, String, PathBuf) {
    compile_path(&fixtures_dir().join(format!("{name}.scala")), name, extra)
}

fn compile_path(src: &Path, tag: &str, extra: &[&str]) -> (bool, String, PathBuf) {
    let out = tmp_dir(tag);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), msgs, out)
}

fn scalac_file(scalac: &Path, src: &Path, tag: &str, cp: Option<&str>) -> (bool, String) {
    let out = tmp_dir(tag);
    let mut cmd = Command::new(scalac);
    if let Some(cp) = cp {
        cmd.args(["-cp", cp]);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    cmd.arg(src);
    let output = cmd.output().expect("run scalac");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&out);
    (output.status.success(), msgs)
}

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// The root, with no jar: a candidate's own parameter settled by its evidence
// clause, where the wanted type only equates it with an unsolved call-site one.
// ---------------------------------------------------------------------------

#[test]
fn a_candidate_parameter_paired_with_an_unsolved_call_site_one_is_settled_by_its_clause() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip gi_tableshape: scala-library jar not present");
        return;
    };
    let (ok, msgs, out) = compile("gi_tableshape", &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "gi_tableshape failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        assert_eq!(
            run_main(&cp),
            expected_stdout("gi_tableshape"),
            "stdout mismatch for gi_tableshape"
        );
    }
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_file(
            &scalac,
            &fixtures_dir().join("gi_tableshape.scala"),
            "gi_tableshape_scalac",
            None,
        );
        assert!(ok, "real scalac rejected gi_tableshape:\n{msgs}");
    }
}

/// Nothing in the fixture needs the real standard library.
#[test]
fn gi_tableshape_runs_under_the_private_runtime() {
    let (ok, msgs, out) = compile("gi_tableshape", &["--no-scala-library"]);
    assert!(ok, "gi_tableshape failed to compile (private):\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&out.display().to_string()),
            expected_stdout("gi_tableshape"),
            "stdout mismatch for gi_tableshape under the private runtime"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The fallback only ever adds a way to *solve* a parameter. A receiver whose
/// evidence clause has no witness is still rejected, and real scalac rejects
/// the same program.
#[test]
fn gi_tableshape_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip gi_tableshape_bad: scala-library jar not present");
        return;
    };
    let (ok, msgs, out) = compile(
        "gi_tableshape_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected gi_tableshape_bad to be rejected:\n{msgs}");
    assert_eq!(
        msgs.matches("could not find implicit value of type Shape")
            .count(),
        1,
        "expected exactly the one search to fail:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_file(
            &scalac,
            &fixtures_dir().join("gi_tableshape_bad.scala"),
            "gi_tableshape_bad_scalac",
            None,
        );
        assert!(!ok, "real scalac accepted gi_tableshape_bad:\n{msgs}");
    }
}

// ---------------------------------------------------------------------------
// The same thing against the published slick jar, which is where the
// gitbucket diagnostics came from.
// ---------------------------------------------------------------------------

/// Fourteen lines, no gitbucket checkout. `r1` and `r3` are the two shapes
/// that failed: a bare table, and a tuple with a table in it. `r2` and `r4`
/// (columns only) always worked, and are here so a regression that breaks them
/// is not mistaken for this root.
const SLICK_MAP: &str = r#"import slick.jdbc.H2Profile.api._

class Accounts(tag: Tag) extends Table[(String, Int)](tag, "ACCOUNT") {
  def name = column[String]("NAME")
  def age = column[Int]("AGE")
  def * = (name, age)
}
object Main {
  val q = TableQuery[Accounts](tag => new Accounts(tag))
  val r1 = q.map(a => a)
  val r2 = q.map(a => a.name)
  val r3 = q.map(a => (a, a.age))
  val r4 = q.map(a => (a.name, a.age))
  def main(args: Array[String]): Unit = println("ok")
}
"#;

#[test]
fn slick_map_over_a_table_finds_its_shape() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip slick_map: scala-library jar not present");
        return;
    };
    let Some(jars) = slick_jars() else {
        eprintln!("skip slick_map: slick 3.4.1 not in the Coursier cache");
        return;
    };
    let dir = tmp_dir("slickmap-src");
    let src = dir.join("SlickMap.scala");
    fs::write(&src, SLICK_MAP).unwrap();
    let cp = classpath(&jars);
    let (ok, msgs, out) = compile_path(
        &src,
        "slickmap",
        &["-cp", &cp, "--scala-library", jar.to_str().unwrap()],
    );
    assert!(
        !msgs.contains("could not find implicit value of type Shape"),
        "the table shapes were not found:\n{msgs}"
    );
    assert!(ok, "slick map fixture failed to compile:\n{msgs}");
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_file(&scalac, &src, "slickmap_scalac", Some(&cp));
        assert!(ok, "real scalac rejected the slick map fixture:\n{msgs}");
    }
    let _ = fs::remove_dir_all(&dir);
}
