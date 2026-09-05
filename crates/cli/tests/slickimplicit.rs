//! E2E tests for the `agent/slickimplicit` slice: the two roots under
//! gitbucket's slick implicit clusters (`Shape`, `TypedType`,
//! `BaseColumnType`). `tests/gitbucket_measure.sh` goes
//! **`errors=1399 → 1276`** (1399 → 1380 for root A alone, 1380 → 1276 for
//! root B on top of it); slick, cats and the scala library are unchanged.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. The jar-backed halves use the published
//! `slick_2.13-3.4.1.jar` from the local Coursier cache and skip when it is
//! not there, the way `crates/cli/tests/slickshape.rs` does.
//!
//! **Root A -- a candidate's type parameter opposite a `_` was never solved.**
//! slick's `anyToShapedValue[T, U](value: T)(implicit shape:
//! Shape[_ <: FlatShapeLevel, T, U, _]): ShapedValue[T, U]` is the conversion
//! behind every `def * = (a, b).mapTo[M]` in a table. `tuple2Shape[Level, M1,
//! M2, U1, U2, P1, P2]` answers it, and its `P1`/`P2` stand opposite the
//! trailing `_`: `Unify` binds nothing there, and `implicit_solve` drops a
//! candidate with a type parameter left undetermined. `implicit_fit_open` is
//! exactly the fallback that settles such parameters from the candidate's
//! *own* implicit clause (`u1`/`u2` say what `P1`/`P2` are), but it refused to
//! run unless the **call site** had left something undetermined. It now runs
//! whenever the ordinary solve failed; every other guard it had is unchanged,
//! so a rule the wanted type says nothing about, or one whose clause has no
//! witness, is still not a candidate.
//!
//! **Root B -- an abstract type member's own type parameters were dropped.**
//! slick's profile cake declares
//!
//! ```text
//! type ColumnType[T] <: TypedType[T]
//! type BaseColumnType[T] <: ColumnType[T] with BaseTypedType[T]
//! ```
//!
//! and nsc pickles those as a `PolyType` over the bounds.
//! `PickleSupply::abstract_type_member` read the bounds and threw the
//! parameters away, so `BaseColumnType` "does not take type parameters" and
//! its bound mentioned a `T` nothing could stand for. On top of that,
//! `conv_ref` only offered a bare `Ref` to `self_type_member` when it had **no
//! arguments**, and `self_type_member` only ran for `scala.*` classes at all
//! -- so `BaseColumnType[Boolean]`, which is how `ImplicitColumnTypes`
//! declares all twenty-four of slick's column types, was an unmappable result
//! type, and so was gitbucket's own `implicit val dateColumnType:
//! BaseColumnType[java.util.Date]`. The member is now installed with its
//! parameters and used as `Applied { ctor: TypeMember, args }`, which
//! `is_sub_type` already substitutes the bound for.
//!
//! **Not fixed, and deliberately pinned as such below:**
//!
//! * a *recursive* derivation whose leftover parameters are still open --
//!   `Shape[_ <: FlatShapeLevel, ((Rep[A], Rep[B]), Rep[C]), ((A, B), C), _]`.
//!   `Unify` keys its unknowns by symbol id, and when `tuple2Shape` derives
//!   `tuple2Shape` the candidate's own `P1` and the caller's open `P1` are the
//!   *same symbol*: the occurs check then rejects `P1 := (P1, P2)` and the
//!   candidate is dropped. nsc gives each application fresh type variables.
//!   One nested-tuple site is left in gitbucket.
//! * `MappedColumnType.base[T, U]`'s own `U : BaseColumnType` when `U` is not
//!   one of slick's built-ins (`java.sql.Timestamp`). The prefix a cake type
//!   member is named through decides whether it is the abstract declaration or
//!   the profile's concrete alias, and this reader does not carry the prefix:
//!   it lands on the abstract one, which nothing can conform *to*. Worth one
//!   error in gitbucket (`model/Profile.scala`), and the value it declares is
//!   usable regardless because its type is written out.

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
        "scala-rs-slickimplicit-{tag}-{}-{nanos}-{seq}",
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

fn jar_tool() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(home).join("bin/jar");
        if p.is_file() {
            return Some(p);
        }
    }
    let out = Command::new("which").arg("jar").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    p.is_file().then_some(p)
}

fn pack_jar(classes: &Path, dest: &Path) {
    let tool = jar_tool().expect("jar tool");
    let out = Command::new(tool)
        .args([
            "cf",
            dest.to_str().unwrap(),
            "-C",
            classes.to_str().unwrap(),
        ])
        .arg(".")
        .output()
        .expect("run jar");
    assert!(
        out.status.success(),
        "jar failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// slick 3.4.1 and the jars it needs, from the local Coursier cache if they
/// happen to be there. Nothing is downloaded. Same list as
/// `crates/cli/tests/slickshape.rs`.
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
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
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

fn scalac_run(scalac: &Path, name: &str, cp: Option<&str>) -> (bool, String) {
    let out = tmp_dir(name);
    let mut cmd = Command::new(scalac);
    if let Some(cp) = cp {
        cmd.args(["-cp", cp]);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    cmd.arg(fixtures_dir().join(format!("{name}.scala")));
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
// Root A: a candidate's type parameter opposite a `_`.
// ---------------------------------------------------------------------------

/// slick's `Shape` derivation written out with no jar and no slick: the
/// witness settles `P1`/`P2` (opposite the wanted type's `_`) and the
/// conversion's `U` (which nothing at the call site pins) from its own
/// implicit clause.
#[test]
fn a_candidates_open_parameter_is_settled_by_its_own_implicit_clause() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip si_shapefit: scala-library jar not present");
        return;
    };
    let (ok, msgs, out) = compile("si_shapefit", &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "si_shapefit failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        assert_eq!(
            run_main(&cp),
            expected_stdout("si_shapefit"),
            "stdout mismatch for si_shapefit"
        );
    }
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_run(&scalac, "si_shapefit", None);
        assert!(ok, "real scalac rejected si_shapefit:\n{msgs}");
    }
}

/// Nothing in the fixture needs the real standard library.
#[test]
fn si_shapefit_runs_under_the_private_runtime() {
    let (ok, msgs, out) = compile("si_shapefit", &["--no-scala-library"]);
    assert!(ok, "si_shapefit failed to compile (private):\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&out.display().to_string()),
            expected_stdout("si_shapefit"),
            "stdout mismatch for si_shapefit under the private runtime"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Running the open-parameter fallback for every failed solve only ever adds a
/// way to *solve* one. A derivation whose own clause has no witness, a wanted
/// type no rule answers, and one whose unpacked type disagrees with the
/// witness are all still rejected -- and real scalac rejects the same three.
#[test]
fn si_shapefit_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip si_shapefit_bad: scala-library jar not present");
        return;
    };
    let (ok, msgs, out) = compile(
        "si_shapefit_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected si_shapefit_bad to be rejected:\n{msgs}");
    assert_eq!(
        msgs.matches("could not find implicit value of type Shape")
            .count(),
        3,
        "expected exactly the three searches to fail:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_run(&scalac, "si_shapefit_bad", None);
        assert!(!ok, "real scalac accepted si_shapefit_bad:\n{msgs}");
        assert_eq!(
            msgs.matches("could not find implicit value for parameter e: Shape")
                .count(),
            3,
            "real scalac rejected si_shapefit_bad for other reasons:\n{msgs}"
        );
    }
}

// ---------------------------------------------------------------------------
// Root B: an abstract type member's own type parameters.
// ---------------------------------------------------------------------------

/// slick's `RelationalTypesComponent` in miniature. The point is the pickle:
/// `stringType`'s result type is written `BaseColumnType[String]`, a bare
/// reference to a *parameterised* abstract member of the cake, which is the
/// only way slick ever spells it.
const B_LIB: &str = r#"
package clib

trait TypedTypeLike[T] { def label: String }
trait BaseTypedTypeLike[T] extends TypedTypeLike[T]

trait TypesComponent {
  type ColumnType[T] <: TypedTypeLike[T]
  type BaseColumnType[T] <: ColumnType[T] with BaseTypedTypeLike[T]

  implicit def stringType: BaseColumnType[String]
  def mk[T](l: String): BaseColumnType[T]
}

class Concrete extends TypesComponent {
  type ColumnType[T] = TypedTypeLike[T]
  type BaseColumnType[T] = TypedTypeLike[T] with BaseTypedTypeLike[T]
  def mk[T](l: String): BaseColumnType[T] = new BaseTypedTypeLike[T] { def label = l }
  implicit def stringType: BaseColumnType[String] = mk[String]("string")
}

object U {
  def label[T](implicit t: TypedTypeLike[T]): String = t.label
}
"#;

/// The declaration is only usable once its result type maps: the bound
/// `ColumnType[T] with BaseTypedTypeLike[T]` is what makes it answer a
/// `TypedTypeLike[String]`.
const B_USER: &str = r#"
import clib._

class Comp {
  val profile: TypesComponent = new Concrete
  import profile._

  def stringLabel: String = U.label[String]
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Comp().stringLabel)
  }
}
"#;

/// Reading the parameters is not the same as accepting anything under the
/// member's name.
const B_USER_BAD: &str = r#"
import clib._

class BadComp {
  val profile: TypesComponent = new Concrete
  import profile._

  val wrong: BaseColumnType[String] = "not a column type"
  val arity: BaseColumnType[String, Int] = null
}
"#;

fn build_b_lib_jar(dir: &Path, scalac: &Path) -> PathBuf {
    let src = dir.join("clib.scala");
    fs::write(&src, B_LIB).unwrap();
    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();
    let out = Command::new(scalac)
        .arg("-d")
        .arg(&lib_out)
        .arg(&src)
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac failed on the miniature cake:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lib_jar = dir.join("clib.jar");
    pack_jar(&lib_out, &lib_jar);
    lib_jar
}

fn compile_source(src: &Path, out: &Path, jar: &Path, cp: &Path) -> (bool, String) {
    let output = Command::new(bin())
        .arg("compile")
        .arg(src)
        .args(["-d", out.to_str().unwrap()])
        .args(["-cp", cp.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

#[test]
fn a_parameterised_abstract_type_member_read_from_a_pickle_keeps_its_parameters() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), real_scalac()) else {
        eprintln!("skip clib: scala-library jar or scalac not present");
        return;
    };
    if jar_tool().is_none() {
        eprintln!("skip clib: no `jar` tool");
        return;
    }
    let dir = tmp_dir("clib");
    let lib_jar = build_b_lib_jar(&dir, &scalac);

    let user = dir.join("cuser.scala");
    fs::write(&user, B_USER).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile_source(&user, &out, &jar, &lib_jar);
    assert!(ok, "the cake's user failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    if java_available() {
        let cp = format!("{}:{}:{}", out.display(), lib_jar.display(), jar.display());
        assert_eq!(run_main(&cp), "string\n", "stdout mismatch for the cake");
    }

    let bad = dir.join("cuserbad.scala");
    fs::write(&bad, B_USER_BAD).unwrap();
    let bad_out = dir.join("badout");
    fs::create_dir_all(&bad_out).unwrap();
    let (ok, msgs) = compile_source(&bad, &bad_out, &jar, &lib_jar);
    assert!(!ok, "expected the bad cake user to be rejected:\n{msgs}");
    assert!(
        msgs.contains("type mismatch"),
        "expected a type mismatch for the wrong value:\n{msgs}"
    );
    assert!(
        msgs.contains("expected 1, found 2"),
        "expected an arity error for `BaseColumnType[String, Int]`:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The same root against the real thing: gitbucket's `model/Profile.scala`
/// shape, an `implicit val` declared at `BaseColumnType[T]` and reached
/// through a self type by a component that writes `column[Event]`.
#[test]
fn slicks_column_types_resolve_through_its_published_jar() {
    let Some(lib) = scala_library_jar() else {
        eprintln!("skip si_coltype_jar: scala-library jar not present");
        return;
    };
    let Some(jars) = slick_jars() else {
        eprintln!("skip si_coltype_jar: slick 3.4.1 not in the local Coursier cache");
        return;
    };
    let cp = classpath(&jars);
    let (ok, msgs, out) = compile(
        "si_coltype_jar",
        &["--scala-library", lib.to_str().unwrap(), "-cp", cp.as_str()],
    );
    assert!(ok, "si_coltype_jar failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_run(&scalac, "si_coltype_jar", Some(&cp));
        assert!(ok, "real scalac rejected si_coltype_jar:\n{msgs}");
    }
}

#[test]
fn si_coltype_jar_bad_is_still_rejected() {
    let Some(lib) = scala_library_jar() else {
        eprintln!("skip si_coltype_jar_bad: scala-library jar not present");
        return;
    };
    let Some(jars) = slick_jars() else {
        eprintln!("skip si_coltype_jar_bad: slick 3.4.1 not in the local Coursier cache");
        return;
    };
    let cp = classpath(&jars);
    let (ok, msgs, out) = compile(
        "si_coltype_jar_bad",
        &["--scala-library", lib.to_str().unwrap(), "-cp", cp.as_str()],
    );
    assert!(!ok, "expected si_coltype_jar_bad to be rejected:\n{msgs}");
    for want in [
        "type mismatch",
        "expected 1, found 2",
        "could not find implicit value of type TypedType[Unmapped]",
    ] {
        assert!(msgs.contains(want), "expected {want:?} in:\n{msgs}");
    }
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_run(&scalac, "si_coltype_jar_bad", Some(&cp));
        assert!(!ok, "real scalac accepted si_coltype_jar_bad:\n{msgs}");
        assert!(
            msgs.contains("wrong number of type arguments") && msgs.contains("TypedType[Unmapped]"),
            "real scalac rejected si_coltype_jar_bad for other reasons:\n{msgs}"
        );
    }
}
