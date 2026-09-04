//! E2E tests for the `agent/lazysig2` slice: a **`def` signature written under
//! an import that only another unit's signature pass settles**.
//!
//! ```scala
//! // unit B, first on the command line
//! trait Comp { self: Prof =>
//!   import profile.api._
//!   def keys(x: Rep[Int]): Rep[Int] = x
//!   def label(n: Int)(implicit s: Session): String = s + n
//! }
//! // unit A, second
//! class MyProfile { object api { type Rep[T] = List[T]; type Session = String } }
//! trait Prof { val profile: MyProfile }
//! ```
//!
//! `profile` belongs to `Prof`, which the component only reaches through its
//! self type, and `Prof` is in a unit whose own signature pass has not run.
//! So `import profile.api._` records no wildcard owner, and `sig_done` made
//! that permanent. nsc has a lazy completer on every symbol and simply forces
//! `profile` when the import is looked at; real scalac 2.13.16 compiles the
//! program above in that order.
//!
//! `agent/slickimpl` had already fixed the `val` half of this
//! (`leave_sig_for_body_pass`). Three things were in the way of the `def`
//! half, and all three are what this file pins down:
//!
//! 1. **`type_def_sig` was not idempotent.** A view or context bound desugars
//!    to an implicit clause that is *appended* to `vparamss`, so building the
//!    signature twice appended a second one and re-typed the first as an
//!    ordinary clause -- whose parameters have no written type at all.
//!    `drop_synthesized_evidence` removes it on the way in. `render[T: Show]`
//!    below is that case.
//! 2. **The first attempt's diagnostics outlived it.** Taking a signature back
//!    left what it had already reported in place, so `val empty: Rep[Int]`
//!    still said "not found: type Rep" even though the rebuild resolved it.
//! 3. **The rebuild happened too late.** It was left to the *body* pass, which
//!    only helps when the caller's unit comes after the callee's. gitbucket's
//!    `controller/` sorts before `service/`, so every controller was typed
//!    against the signature that had been taken back. The rebuild is now a
//!    second signature round over every unit, before any body is typed.
//!
//! `label`'s `(implicit s: Session)` is the one that costs the most and shows
//! the least: a bare `Ident` in a parameter position is not under
//! `strict_type_names`, so an unresolved one becomes a silent placeholder and
//! the *definition* reports nothing. Every caller then reports
//! "could not find implicit value of type Session" instead. That is what
//! gitbucket's 187 such diagnostics were, beside the 36 `not found: type Rep`
//! from the applied types in the very same methods (`docs/gitbucket.md`).
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts, and the
//! sources are written from here rather than into `tests/fixtures/` because
//! the bug only exists across two units; see `crates/cli/tests/aliaslookup.rs`
//! for the same arrangement.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-lazysig2-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// The component: written under `import profile.api._`, where `profile` is a
/// member of the self type and lives in the *other* unit.
const COMP: &str = r#"
package plib

trait Comp { self: Prof =>
  import profile.api._

  // An applied type in a `def` signature. This half reported
  // "not found: type Rep" at the definition.
  def keys(x: Rep[Int]): Rep[Int] = x
  // A bare name in a parameter position. This half reported nothing here and
  // "could not find implicit value of type Session" at every caller.
  def label(n: Int)(implicit s: Session): String = s + n
  // A context bound: `type_def_sig` appends the implicit clause it desugars
  // to, so rebuilding the signature must not append a second one.
  def render[T: Show](x: T): Rep[String] = List(implicitly[Show[T]].show(x))
  // The `val` half, which `agent/slickimpl` already reached.
  val empty: Rep[Int] = Nil
}
"#;

/// The unit that gives `profile` its type, and the program that runs it.
const LIB: &str = r##"
package plib

class MyProfile {
  object api {
    type Rep[T] = List[T]
    type Session = String
  }
}

trait Prof {
  val profile: MyProfile
}

trait Show[T] { def show(x: T): String }
object Show {
  implicit val showInt: Show[Int] = new Show[Int] { def show(x: Int): String = "#" + x }
}

object Runner extends Comp with Prof {
  val profile: MyProfile = new MyProfile
}

object Main {
  def main(args: Array[String]): Unit = {
    implicit val s: String = "s"
    println(Runner.keys(List(1, 2)))
    println(Runner.label(3))
    println(Runner.render(7))
    println(Runner.empty)
  }
}
"##;

/// Rebuilding a signature must not swallow what is wrong with it, and must not
/// report it twice either. Real scalac reports all three of these.
const BAD: &str = r#"
package plib

trait BadComp { self: Prof =>
  import profile.api._

  def oops(x: Missing[Int]): Rep[Int] = Nil
  def wrongTy(x: Rep[Int]): Rep[String] = x
}

object BadCall {
  def go: String = Runner.label(3)
}
"#;

const EXPECTED: &str = "List(1, 2)\ns3\nList(#7)\nList()\n";

fn write_sources(dir: &Path, with_bad: bool) -> Vec<PathBuf> {
    // The component comes *first*, which is the whole point: its import
    // cannot be resolved until the unit behind it has been walked.
    let comp = dir.join("pcomp.scala");
    let lib = dir.join("plib.scala");
    fs::write(&comp, COMP).unwrap();
    fs::write(&lib, LIB).unwrap();
    let mut srcs = Vec::new();
    if with_bad {
        let bad = dir.join("pbad.scala");
        fs::write(&bad, BAD).unwrap();
        srcs.push(bad);
    }
    srcs.push(comp);
    srcs.push(lib);
    srcs
}

fn compile(dir: &Path, srcs: &[PathBuf], jar: &Path) -> (bool, String, PathBuf) {
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .args(srcs)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs, out)
}

fn run_main(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "plib.Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java plib.Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_def_signature_under_an_import_another_unit_settles_is_built_again() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip lazysig2: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("good");
    let srcs = write_sources(&dir, false);
    let (ok, msgs, out) = compile(&dir, &srcs, &jar);
    assert!(ok, "lazysig2 failed to compile:\n{msgs}");
    assert!(
        !msgs.contains("not found: type Rep"),
        "the import was not reached on the second signature round:\n{msgs}"
    );
    if java_available() {
        assert_eq!(run_main(&out, &jar), EXPECTED, "{msgs}");
    }
}

/// The same sources through real scalac 2.13.16, in the same order, so the
/// claim "this program is legal" is not ours to make.
#[test]
fn real_scalac_accepts_the_same_two_units_in_the_same_order() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip lazysig2 dual-run: scalac or scala-library not present");
        return;
    };
    if !java_available() {
        eprintln!("skip lazysig2 dual-run: no java");
        return;
    }
    let dir = tmp_dir("scalac");
    let srcs = write_sources(&dir, false);
    let out = dir.join("refout");
    fs::create_dir_all(&out).unwrap();
    let output = Command::new(&scalac)
        .arg("-d")
        .arg(&out)
        .args(&srcs)
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "real scalac rejected the reproduction:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_main(&out, &jar), EXPECTED);
}

#[test]
fn rebuilding_a_signature_neither_swallows_nor_repeats_its_diagnostics() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip lazysig2_bad: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("bad");
    let srcs = write_sources(&dir, true);
    let (ok, msgs, _out) = compile(&dir, &srcs, &jar);
    assert!(!ok, "lazysig2_bad compiled:\n{msgs}");
    // Reported once, by the round that built the signature that stands.
    assert_eq!(
        msgs.matches("not found: type Missing").count(),
        1,
        "expected exactly one `not found: type Missing`:\n{msgs}"
    );
    assert!(
        msgs.contains("type mismatch"),
        "expected the `Rep[Int]` / `Rep[String]` mismatch:\n{msgs}"
    );
    assert!(
        msgs.contains("could not find implicit value"),
        "expected the missing `Session` at the call:\n{msgs}"
    );
}
