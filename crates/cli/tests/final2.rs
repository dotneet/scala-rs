//! E2E tests for the `agent/final2` slice: the three cats-effect corners slick
//! still failed on.
//!
//! All three were reported as "does not reproduce on its own", and all three
//! have the same shape underneath — a symbol reaches the table by one route
//! before the program names it by another, and the first route's answer is the
//! one that sticks:
//!
//! 1. **`Ref.of` could not find its `Ref.Make[F]`.** Every witness for it is
//!    inherited into the companion of the *nested* trait `Ref.Make`, and
//!    `Check::load_companion_module` installed `cats/effect/kernel/Ref$Make$`
//!    in the *package* `cats.effect.kernel` under the name `Make`.
//!    `SymbolTable::companion_module` looks for a module of the same name among
//!    the class's own owner's members — `Ref`, not the package — so `Make`'s
//!    implicit scope was empty. Writing `Ref.Make` anywhere in the source built
//!    the companion by another route and made the search work, which is what
//!    made this look order-dependent. slick's `basic/ConcurrencyControl.scala`.
//!
//! 2. **`Resource.ExitCase` was "not a member of Resource$".** A nested class
//!    file `Outer$Inner` does not say whether `class Outer` or `object Outer`
//!    declares it, and `classpath::java_class_owner` always answers the class.
//!    Reading `fs2/Stream.class` mentions `cats/effect/kernel/Resource$ExitCase`
//!    in a member descriptor, so the symbol was entered under the *trait*
//!    `Resource` — and the source's `Resource.ExitCase`, a path through the
//!    `Resource` **object**, looked it up on `Resource$` and found nothing.
//!    `BasicBackend.scala` on its own got the other order and compiled.
//!    slick's `basic/BasicBackend.scala`.
//!
//! 3. **`cats.effect.IO(fa)` was "no matching overload".** A class file cannot
//!    write a by-name parameter: `IO.apply(thunk: => A)` reads back as
//!    `apply(Function0[A]): IO[A]`, both on the companion and as a static
//!    forwarder on the class. The on-demand pickle path never corrects that,
//!    because it runs only when a lookup finds *nothing*. slick's
//!    `dbio/DBIOAction.scala`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `f2` prefix.

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
        "scala-rs-final2-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// The jars slick's `basic/` and `dbio/` are compiled against, from the local
/// Coursier cache if they happen to be there. Nothing is downloaded; the tests
/// skip when a jar is missing. Same shape as `crates/cli/tests/tail6.rs`.
fn slick_effect_jars() -> Option<Vec<PathBuf>> {
    let home = std::env::var("HOME").ok()?;
    let roots = [
        PathBuf::from(&home).join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        PathBuf::from(&home).join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ];
    let wanted = [
        ("org/typelevel/cats-core_2.13", "cats-core_2.13"),
        ("org/typelevel/cats-kernel_2.13", "cats-kernel_2.13"),
        (
            "org/typelevel/cats-effect-kernel_2.13",
            "cats-effect-kernel_2.13",
        ),
        ("org/typelevel/cats-effect_2.13", "cats-effect_2.13"),
        ("org/typelevel/cats-effect-std_2.13", "cats-effect-std_2.13"),
        ("org/typelevel/cats-mtl_2.13", "cats-mtl_2.13"),
        ("org/scodec/scodec-bits_2.13", "scodec-bits_2.13"),
        ("co/fs2/fs2-core_2.13", "fs2-core_2.13"),
    ];
    let mut out = Vec::new();
    for (rel, prefix) in wanted {
        let mut found = None;
        for root in &roots {
            let Ok(rd) = fs::read_dir(root.join(rel)) else {
                continue;
            };
            for ent in rd.flatten() {
                let version = ent.file_name().to_string_lossy().into_owned();
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

/// Compile one fixture against the real jars. Answers (success, diagnostics).
fn compile_fixture(name: &str, lib: &Path, jars: &[PathBuf]) -> (bool, String) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            &classpath(jars),
            "--scala-library",
            lib.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&out);
    (output.status.success(), msgs)
}

/// All three cases in one file, exactly as slick spells them. Every one of them
/// failed on `main`; the `Resource.ExitCase` and `cats.effect.IO(fa)` ones only
/// because the same file also names `fs2.Stream`, whose class file is what
/// enters `Resource$ExitCase` and `IO`'s erased members first.
#[test]
fn the_cats_effect_corners_slick_needs_typecheck() {
    let Some(lib) = scala_library_jar() else {
        eprintln!("skip f2_cats: scala-library jar not present");
        return;
    };
    let Some(jars) = slick_effect_jars() else {
        eprintln!("skip f2_cats: cats-effect / fs2 jars not in the local Coursier cache");
        return;
    };
    let (ok, msgs) = compile_fixture("f2_cats", &lib, &jars);
    assert!(ok, "f2_cats failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
}

/// The same three shapes, each written so real scalac rejects it too. A fix
/// that made these compile would be a fix that resolves anything.
#[test]
fn the_neighbouring_rejections_still_stand() {
    let Some(lib) = scala_library_jar() else {
        eprintln!("skip f2_cats_bad: scala-library jar not present");
        return;
    };
    let Some(jars) = slick_effect_jars() else {
        eprintln!("skip f2_cats_bad: cats-effect / fs2 jars not in the local Coursier cache");
        return;
    };
    let (ok, msgs) = compile_fixture("f2_cats_bad", &lib, &jars);
    assert!(!ok, "expected f2_cats_bad to be rejected, got:\n{msgs}");
    for want in [
        // No `Async` / `Sync` in scope: nothing derives `Ref.Make[F]`.
        "could not find implicit value of type Make[F]",
        // The `Resource` object really has no such type member.
        "type NoSuchCase is not a member of Resource$",
        // `IO.fromFuture` wants an `IO[Future[A]]`.
        "with arguments (IO[Int])",
    ] {
        assert!(msgs.contains(want), "expected {want:?} in:\n{msgs}");
    }
}

fn real_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

/// The dual run: real scalac 2.13.16 accepts the good fixture and rejects the
/// bad one, at the same three places. Skipped when the pinned compiler is not
/// unpacked (`tests/slick_measure.sh` puts it there).
#[test]
fn real_scalac_agrees_on_both_fixtures() {
    let Some(scalac) = real_scalac() else {
        eprintln!("skip scalac dual run: /tmp/scala-2.13.16 not present");
        return;
    };
    let Some(jars) = slick_effect_jars() else {
        eprintln!("skip scalac dual run: cats-effect / fs2 jars not in the local Coursier cache");
        return;
    };
    for (name, should_compile) in [("f2_cats", true), ("f2_cats_bad", false)] {
        let out = tmp_dir(name);
        let output = Command::new(&scalac)
            .args([
                "-classpath",
                &classpath(&jars),
                "-d",
                out.to_str().unwrap(),
                fixtures_dir()
                    .join(format!("{name}.scala"))
                    .to_str()
                    .unwrap(),
            ])
            .output()
            .expect("run scalac");
        let msgs = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.status.success(),
            should_compile,
            "scalac disagrees on {name}:\n{msgs}"
        );
        if !should_compile {
            assert!(
                msgs.contains("Cannot find an instance for Ref.Make")
                    && msgs.contains("type NoSuchCase is not a member")
                    && msgs.contains("required: scala.concurrent.Future[Int]"),
                "scalac rejected {name} for other reasons:\n{msgs}"
            );
        }
        let _ = fs::remove_dir_all(&out);
    }
}
