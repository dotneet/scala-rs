//! E2E tests for the `agent/thiscast` slice: a receiver the assembler already
//! tracks as reaching the callee's owner needs no `checkcast`.
//!
//! `checkcast_erased_method_receiver` in `crates/backend/src/gen.rs` cast the
//! receiver of every non-static, non-super, non-module, non-value-class call to
//! the callee's owner, unconditionally. For the overwhelmingly common case --
//! `this.m()` inside the class that declares `m` -- that is three wasted bytes
//! per call:
//!
//! ```text
//! scala-rs:  aload_0; checkcast C; invokevirtual C.m   (7 bytes)
//! nsc:       aload_0; invokevirtual C.m                (4 bytes)
//! ```
//!
//! It is not only size. `scala/test/files/run/t10594.scala` came out at 57925
//! bytes against nsc's 33109 -- 43% larger -- and since the previous slice
//! turned `Method too large` (JVMS 4.7.3, `code_length < 65536`) into a
//! diagnostic rather than a silently truncated class, the 43% decides whether a
//! method nsc accepts compiles here at all. `a_method_nsc_accepts_is_accepted`
//! below is the shape that regressed: 12500 calls of `this.m()`, which nsc
//! encodes in 50000 bytes and we encoded in 87500.
//!
//! The fix asks `Assembler::top_object` -- the same model the StackMapTable is
//! written from -- what the verifier will see on the stack, and skips the cast
//! when `jvm_assignable` says that class already reaches the owner. Everything
//! it cannot resolve keeps its cast, so the receiver shapes that genuinely need
//! one are unaffected; `tk_thiscast.scala` pins both directions, and
//! `self_type_receiver_keeps_its_cast` checks the byte sequence against what
//! real scalac 2.13.16 emits for the same source.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.
//!
//! Fixture prefix: `tk_`.

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
        "scala-rs-thiscast-{tag}-{}-{nanos}-{seq}",
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

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile_source(out: &Path, jar: Option<&Path>, src: &Path) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    cmd.arg(src);
    cmd.args(["-d", out.to_str().unwrap()]);
    match jar {
        Some(j) => cmd.args(["--scala-library", j.to_str().unwrap()]),
        None => cmd.arg("--no-scala-library"),
    };
    let output = cmd.output().expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

fn run_main(out: &Path, jar: Option<&Path>, main: &str) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn disassemble(out: &Path, class: &str) -> String {
    let output = Command::new("javap")
        .args(["-c", "-p", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    assert!(
        output.status.success(),
        "javap {class} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The body of one method as `javap -c` prints it, without the signature line.
fn method_body(disasm: &str, signature: &str) -> String {
    let start = disasm
        .find(signature)
        .unwrap_or_else(|| panic!("no `{signature}` in:\n{disasm}"));
    let rest = &disasm[start + signature.len()..];
    let end = rest.find("\n\n").unwrap_or(rest.len());
    rest[..end].to_string()
}

fn compile_fixture(jar: Option<&Path>) -> PathBuf {
    let src = fixtures_dir().join("tk_thiscast.scala");
    let out = tmp_dir("fixture");
    let (ok, msgs) = compile_source(&out, jar, &src);
    assert!(ok, "compile tk_thiscast failed: {msgs}");
    out
}

/// The fixture runs the same in both modes, and the same as real scalac.
#[test]
fn tk_thiscast_runs() {
    if !java_available() {
        eprintln!("skip: no java");
        return;
    }
    let want = expected_stdout("tk_thiscast");

    let out = compile_fixture(None);
    assert_eq!(run_main(&out, None, "tk.Main"), want, "private-runtime run");
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture(Some(&jar));
    assert_eq!(
        run_main(&out, Some(&jar), "tk.Main"),
        want,
        "library dual-run"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `this.m()` where `this` already reaches the owner emits no `checkcast`, in
/// `C` (a class reaching its own methods, its superclass's and its mixins')
/// and in `Mix` (a trait reaching its own).
#[test]
fn a_receiver_that_already_reaches_the_owner_is_not_cast() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: no scala-library jar");
        return;
    };
    if !javap_available() {
        eprintln!("skip: no javap");
        return;
    }
    let out = compile_fixture(Some(&jar));

    let c = disassemble(&out, "tk.C");
    assert!(
        !c.contains("checkcast"),
        "tk.C still casts a receiver it does not have to:\n{c}"
    );
    let mix = disassemble(&out, "tk.Mix");
    assert!(
        !mix.contains("checkcast"),
        "tk.Mix still casts a receiver it does not have to:\n{mix}"
    );

    // Precisely: the four bytes nsc emits, not seven.
    let all = method_body(&c, "public java.lang.String all();");
    assert!(
        all.contains("aload_0") && all.contains("invokevirtual") && !all.contains("checkcast"),
        "tk.C.all() is not `aload_0; invokevirtual`:\n{all}"
    );

    let _ = fs::remove_dir_all(&out);
}

/// The cast that is *not* redundant survives: inside `trait T { self: U => }`
/// the receiver is a `T`, which is not a `U`, so `um()` needs `checkcast U`.
/// Real scalac emits the same two instructions, so compare against it when it
/// is installed.
#[test]
fn self_type_receiver_keeps_its_cast() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: no scala-library jar");
        return;
    };
    if !javap_available() {
        eprintln!("skip: no javap");
        return;
    }
    let out = compile_fixture(Some(&jar));
    let t = disassemble(&out, "tk.T");
    let ours = method_body(&t, "viaSelf();");
    assert!(
        ours.contains("checkcast") && ours.contains("class tk/U"),
        "tk.T.viaSelf() lost the cast the self type needs:\n{ours}"
    );
    // Two calls in the body, two casts.
    assert_eq!(
        ours.matches("checkcast").count(),
        2,
        "tk.T.viaSelf() should cast both receivers:\n{ours}"
    );

    if let Some(scalac) = find_scalac() {
        let nsc_out = tmp_dir("nsc-selftype");
        let status = Command::new(scalac)
            .args(["-d", nsc_out.to_str().unwrap()])
            .arg(fixtures_dir().join("tk_thiscast.scala"))
            .status()
            .expect("run scalac");
        assert!(status.success(), "scalac failed on tk_thiscast.scala");
        let theirs = method_body(&disassemble(&nsc_out, "tk.T"), "viaSelf();");
        assert_eq!(
            theirs.matches("checkcast").count(),
            2,
            "nsc does not cast the self-type receiver after all:\n{theirs}"
        );
        let _ = fs::remove_dir_all(&nsc_out);
    }

    let _ = fs::remove_dir_all(&out);
}

/// The hop the Scala hierarchy makes and the bytecode cannot: `trait TT extends
/// UBase` compiles to an interface, so a `TT` receiver is not assignable to the
/// class `UBase` as far as JVMS 4.10.1.2 is concerned, and the cast in front of
/// `um2()` has to stay.
///
/// The first version of this slice dropped it, because the symbol table's base
/// type sequence answers the *Scala* question. Nine `run` tests of the
/// scala/scala corpus died with `VerifyError: Bad type on operand stack ...
/// Type 'scala/reflect/api/JavaUniverse' is not assignable to
/// 'scala/reflect/api/Universe'` -- `JavaUniverse` is this shape in the wild,
/// an interface whose class file declares `interfaces: 0` while its pickle
/// says it extends the abstract class `Universe`. This fixture is the same
/// shape in three lines of source, and `tk_thiscast_runs` verifies it under
/// `-Xverify:all`.
#[test]
fn a_trait_extending_a_class_keeps_its_cast() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: no scala-library jar");
        return;
    };
    if !javap_available() {
        eprintln!("skip: no javap");
        return;
    }
    let out = compile_fixture(Some(&jar));
    let tt = disassemble(&out, "tk.TT");
    assert!(
        tt.contains("public interface tk.TT"),
        "tk.TT is not an interface, so this test no longer pins the shape:\n{tt}"
    );
    let ours = method_body(&tt, "viaClassParent();");
    assert_eq!(
        ours.matches("checkcast").count(),
        2,
        "tk.TT.viaClassParent() dropped a cast the verifier needs:\n{ours}"
    );
    assert!(
        ours.contains("class tk/UBase"),
        "tk.TT.viaClassParent() casts to something other than tk/UBase:\n{ours}"
    );

    if let Some(scalac) = find_scalac() {
        let nsc_out = tmp_dir("nsc-traitclass");
        let status = Command::new(scalac)
            .args(["-d", nsc_out.to_str().unwrap()])
            .arg(fixtures_dir().join("tk_thiscast.scala"))
            .status()
            .expect("run scalac");
        assert!(status.success(), "scalac failed on tk_thiscast.scala");
        let theirs = method_body(&disassemble(&nsc_out, "tk.TT"), "viaClassParent();");
        assert_eq!(
            theirs.matches("checkcast").count(),
            2,
            "nsc does not cast a trait's class-parent receiver after all:\n{theirs}"
        );
        let _ = fs::remove_dir_all(&nsc_out);
    }

    let _ = fs::remove_dir_all(&out);
}

/// `n` calls of `m()`, a hundred to a line. Same generator as
/// `crates/cli/tests/ms_bigmethod.rs`: a 50 KB fixture would be re-read by
/// every sweep for no extra signal.
fn calls(n: usize) -> String {
    let mut s = String::new();
    for chunk in 0..n.div_ceil(100) {
        let here = (n - chunk * 100).min(100);
        s.push_str("      ");
        for _ in 0..here {
            s.push_str("m();");
        }
        s.push('\n');
    }
    s
}

/// The correctness half. 12500 calls of `this.m()` is 50000 bytes of code for
/// nsc and was 87500 for us -- over the 65535 of JVMS 4.7.3, so we reported
/// `Method too large` and wrote nothing for a class real scalac compiles and
/// runs. Compile it, verify it, run it, and compare with scalac's own stdout.
#[test]
fn a_method_nsc_accepts_is_accepted() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: no scala-library jar");
        return;
    };
    if !java_available() {
        eprintln!("skip: no java");
        return;
    }
    let dir = tmp_dir("bignsc");
    let src = dir.join("TkBigCalls.scala");
    fs::write(
        &src,
        format!(
            "class Big {{\n  var x = 0\n  def m(): Unit = x += 1\n\
             \n  def big(): Unit = {{\n{}  }}\n}}\n\
             \nobject Test {{\n  def main(args: Array[String]): Unit = {{\n\
             \x20   val b = new Big\n    b.big()\n    println(b.x)\n  }}\n}}\n",
            calls(12500)
        ),
    )
    .unwrap();

    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile_source(&out, Some(&jar), &src);
    assert!(
        ok,
        "12500 `this.m()` calls fit in 50000 bytes for nsc; we rejected them: {msgs}"
    );
    assert!(
        out.join("Big.class").exists(),
        "no Big.class was written: {msgs}"
    );
    let got = run_main(&out, Some(&jar), "Test");
    assert_eq!(got, "12500\n");

    if let Some(scalac) = find_scalac() {
        let nsc_out = tmp_dir("bignsc-nsc");
        let status = Command::new(scalac)
            .args(["-d", nsc_out.to_str().unwrap()])
            .arg(&src)
            .status()
            .expect("run scalac");
        assert!(status.success(), "scalac rejected the 12500-call method");
        assert_eq!(
            run_main(&nsc_out, Some(&jar), "Test"),
            got,
            "nsc prints something else"
        );
        let _ = fs::remove_dir_all(&nsc_out);
    }

    let _ = fs::remove_dir_all(&dir);
}

/// The other side of the same limit is unchanged: 20000 calls is 80000 bytes
/// even at four bytes each, and nsc rejects it too. Guards against "fix the
/// size, lose the diagnostic".
#[test]
fn a_method_too_large_for_any_encoding_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: no scala-library jar");
        return;
    };
    let dir = tmp_dir("stilltoolarge");
    let src = dir.join("TkStillTooLarge.scala");
    fs::write(
        &src,
        format!(
            "class Big {{\n  var x = 0\n  def m(): Unit = x += 1\n\
             \n  def big(): Unit = {{\n{}  }}\n}}\n",
            calls(20000)
        ),
    )
    .unwrap();

    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile_source(&out, Some(&jar), &src);
    assert!(!ok, "the compile should fail, but it succeeded: {msgs}");
    assert!(
        msgs.contains("Method too large: Big.big ()V"),
        "no `Method too large`: {msgs}"
    );
    assert!(
        !out.join("Big.class").exists(),
        "a class file was written for a method that cannot be encoded"
    );
    let _ = fs::remove_dir_all(&dir);
}
