//! E2E tests for the `agent/hkfield` slice: **a value whose declared type
//! erases to a bound, rather than to `Object`, was read back without a cast.**
//!
//! `agent/overscore` left this reduced, which is why its own `ovsc_legal.scala`
//! builds a `Holder` and then declines to read the field:
//!
//! ```scala
//! trait Boxy[X] { def get: X }
//! class OneBox[X](val get: X) extends Boxy[X]
//! class Holder[F[X] <: Boxy[X], A](val f: F[A])
//! new Holder[OneBox, Int](new OneBox(3)).f.get
//! ```
//!
//! `Holder.f` has the JVM descriptor `LBoxy;` -- `F`'s bound -- while `h.f` at
//! `Holder[OneBox, Int]` is an `OneBox[Int]`, so reading a member declared on
//! `OneBox` off it put a `Boxy` under `getfield OneBox.get` and the JVM
//! verifier threw the whole method out: `VerifyError: Bad type on operand
//! stack`. Entirely pre-existing, in **both** `--scala-library` and
//! private-runtime modes, and invisible to every compile-time measure -- the
//! class files are emitted, the compiler reports no error, and only the JVM
//! objects.
//!
//! **The defect is not higher-kinded.** `F[A]` erasing to a bound is one
//! instance of it; the first-order `class BHolder[T <: Boxy[Int]](val f: T)` is
//! broken identically on the pre-fix binary, and the fixture pins both. What
//! made it look higher-kinded is that the *unbounded* parameter people usually
//! write erases to `Object`, and `Object` was the one case the load path cast
//! for.
//!
//! **There are two sites, not one.** Fixing the field-read path leaves
//! `case CBox(b, n)` for `case class CBox[F[X] <: Boxy[X], A](f: F[A], n: Int)`
//! still failing the verifier: the match lowering does not go through `Select`
//! at all, it reads the constructor field itself (`gen_ctor_fields_pattern`)
//! and carried its own copy of the same `Object`-only test, so the binder held
//! a `Boxy` while the typer had given it `OneBox[Int]`. Both now ask one shared
//! question, `erased_load_needs_narrowing`. The second site was found by
//! probing the neighbours after the first was green -- which is the standing
//! lesson that a verifier failure marks the extent of what the *verifier* can
//! see, not the extent of the damage.
//!
//! **The fix** is one arm in `maybe_cast_erased_load`, and it is a copy of the
//! question the *method result* path already asks in
//! `maybe_unbox_erased_result`: is the declared erasure *known* to conform to
//! the type the typer settled on? If it cannot be shown to, cast. That is why
//! `MHolder.m` (case 4 in the fixture) was already correct before this slice
//! and the field beside it was not -- the two paths had drifted apart, and this
//! closes the gap rather than inventing a rule.
//!
//! **One known difference from scalac, and it is not new.** scalac's erasure
//! adapts to the *expected* type, so `val hb: Boxy[Int] = h.f` gets no cast at
//! all; this compiler's load path casts to the *tree's* type and emits a
//! harmless, always-succeeding `checkcast OneBox`. The pre-fix binary already
//! does exactly this at `val ob: Boxy[Int] = oholder.f` (the `Object` arm of
//! the same function) and at `val mb: Boxy[Int] = mholder.m` (the method-result
//! sibling), both verified against scalac on the pre-fix binary. This slice
//! makes the bounded case behave like the two arms beside it; narrowing all
//! three to the expected type is a separate change and is not attempted here.
//!
//! Measured on this tree: slick `errors=0 files_with_errors=0 classes=1490`
//! with all 1490 class files **byte-identical** to the pre-fix binary's
//! (`SLICK_OUT` on both saved binaries, `diff -r` empty) -- slick never reads a
//! member off a value declared at a bounded parameter, so nothing there moves.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with the
//! other slices running in parallel; see `.agent-brief.md`.

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
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-hkfield-{tag}-{}-{nanos}-{seq}",
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

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
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
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn run_main(out: &Path, main: &str, cp_extra: &[&str]) -> String {
    let mut cp = out.display().to_string();
    for e in cp_extra {
        cp.push(':');
        cp.push_str(e);
    }
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The `javap -c` disassembly of one class in `out`.
fn disasm(out: &Path, class: &str) -> String {
    let o = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    assert!(
        o.status.success(),
        "javap {class} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// The verifier is the test. Both modes, both against real scalac's output.
//
// On the pre-fix binary `java` refuses `hkf.Main` outright in *both* modes --
// `VerifyError: Bad type on operand stack … Type 'hkf/Boxy' … is not
// assignable to 'hkf/OneBox'` -- so these two tests are the whole claim.
// ---------------------------------------------------------------------------

#[test]
fn fixtures_hkfield_private_runtime() {
    let out = compile_fixture_with("hkfield", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(run_main(&out, "hkf.Main", &[]), expected_stdout("hkfield"));
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_hkfield_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("hkfield", &["--scala-library", jar_s]);
    assert_eq!(
        run_main(&out, "hkf.Main", &[jar_s]),
        expected_stdout("hkfield")
    );
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------------------------------------
// Where the cast goes, and where it must not.
//
// Running green is necessary but not sufficient: a cast emitted where scalac
// omits one still verifies and still runs, and is still a divergence. scalac
// 2.13.16 compiling the same source emits `checkcast hkf/OneBox` immediately
// after each of the five wide loads below, `checkcast hkf/WOne` after the
// higher-kinded-bound one, and *nothing* after `Plain.c`, whose descriptor is
// already `Lhkf/OneBox;`.
// ---------------------------------------------------------------------------

#[test]
fn hkfield_casts_the_wide_loads_and_only_those() {
    if !javap_available() {
        eprintln!("skip: no javap");
        return;
    }
    let out = compile_fixture_with("hkfield", &["--no-scala-library"]);
    let code = disasm(&out, "hkf.Main$");

    // Each wide read is followed by the cast scalac puts there. The pair is
    // matched in order on the instruction listing, so a cast landing anywhere
    // else does not satisfy it.
    for (load, cast) in [
        // (1) the higher-kinded field, and (2) the first-order bounded one:
        // both descriptors are the bound `Lhkf/Boxy;`.
        ("Field hkf/Holder.f:Lhkf/Boxy;", "class hkf/OneBox"),
        ("Field hkf/BHolder.f:Lhkf/Boxy;", "class hkf/OneBox"),
        // (3) the `Object` case, which already worked: still cast.
        ("Field hkf/OHolder.f:Ljava/lang/Object;", "class hkf/OneBox"),
        // (4) the method result, which already worked: still cast.
        ("Method hkf/MHolder.m:()Lhkf/Boxy;", "class hkf/OneBox"),
        // (5) through a pattern, (5b) bound by a case-class extractor -- the
        // second site of the root, in the match lowering rather than the
        // `Select` path -- and (6) at a higher-kinded bound.
        ("Field hkf/PBox.f:Lhkf/Boxy;", "class hkf/OneBox"),
        ("Field hkf/CBox.f:Lhkf/Boxy;", "class hkf/OneBox"),
        ("Field hkf/WHolder.w:Lhkf/Wrap;", "class hkf/WOne"),
    ] {
        let at = code
            .find(load)
            .unwrap_or_else(|| panic!("no `{load}` in hkf.Main$:\n{code}"));
        let rest = &code[at + load.len()..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let next = &rest[line_end..];
        let next_insn: String = next.lines().nth(1).unwrap_or_default().into();
        assert!(
            next_insn.contains("checkcast") && next_insn.contains(cast),
            "`{load}` is not followed by `checkcast {cast}`, but by `{next_insn}`;\n{code}"
        );
    }

    // The over-reach guard. `Plain.c` is declared `OneBox[Int]`, so its
    // descriptor already *is* `Lhkf/OneBox;` and scalac emits no cast. A rule
    // that cast here would be wrong in the other direction.
    let at = code
        .find("Field hkf/Plain.c:Lhkf/OneBox;")
        .unwrap_or_else(|| panic!("no `Plain.c` read in hkf.Main$:\n{code}"));
    let rest = &code[at..];
    let next_insn: String = rest.lines().nth(1).unwrap_or_default().into();
    assert!(
        !next_insn.contains("checkcast"),
        "`Plain.c` must not be cast (scalac emits none), but is followed by `{next_insn}`;\n{code}"
    );

    // `viaParam` reads `get` off `F[A]` directly. `get` is the *bound's* own
    // member, so the receiver already has the right type and scalac casts
    // nothing; nor may we.
    let vp = disasm(&out, "hkf.Main$");
    let body = vp
        .split("viaParam")
        .nth(1)
        .unwrap_or_else(|| panic!("no viaParam in hkf.Main$:\n{vp}"));
    let body = &body[..body.find("\n\n").unwrap_or(body.len())];
    assert!(
        !body.contains("checkcast     hkf/OneBox") && !body.contains("class hkf/OneBox"),
        "viaParam must not cast its parameter to OneBox:\n{body}"
    );

    let _ = fs::remove_dir_all(&out);
}
