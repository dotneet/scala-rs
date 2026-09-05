//! E2E tests for ovl (overload resolution and method application) and
//! exptype (method type parameters solved from the expected type), plus the
//! separate-compilation tests for a package object's mirror class, a value
//! class's companion and an operator-named nested object that the exptype
//! fixture's library exercises. Split out of `e2e.rs`.

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
    // Two tests can share a tag, and the clock is not fine enough to
    // separate them: they ran in the same directory and each `java Main` saw
    // the other's half-written output.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-e2e-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
    assert!(
        out.join("Main$.class").is_file(),
        "Main$.class missing in {}",
        out.display()
    );
    out
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn compile_fails_lib(name: &str, needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip compile_fails_lib {name}: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

const LIBRARY_COLLIDERS: &[&str] = &[
    "scala/Option.class",
    "scala/Some.class",
    "scala/Some$.class",
    "scala/None$.class",
    "scala/Function0.class",
    "scala/Function1.class",
    "scala/PartialFunction.class",
    "scala/Tuple2.class",
    "scala/NotImplementedError.class",
    "scala/collection/immutable/List.class",
    "scala/collection/immutable/$colon$colon.class",
    "scala/collection/immutable/Nil$.class",
    "scala/collection/immutable/List$.class",
    "scala/runtime/ArrowAssoc.class",
    "scala/Predef$.class",
    "scala/collection/StringOps.class",
    "scala/collection/ArrayOps.class",
    "scala/collection/ArrayOps$.class",
    "scala/collection/WithFilter.class",
    "scala/collection/Iterator.class",
    "scala/Option$WithFilter.class",
    "scala/collection/immutable/Map.class",
    "scala/collection/immutable/Map$.class",
    "scala/collection/immutable/Vector.class",
    "scala/collection/immutable/Vector$.class",
    "scala/collection/immutable/IndexedSeq.class",
    "scala/collection/immutable/IndexedSeq$.class",
    "scala/collection/immutable/Queue.class",
    "scala/collection/immutable/Queue$.class",
    "scala/Predef$any2stringadd.class",
    "scala/Predef$ArrowAssoc.class",
    "scala/runtime/RichInt.class",
    "scala/runtime/RichLong.class",
    "scala/runtime/RichDouble.class",
    "scala/runtime/RichChar.class",
    "scala/collection/immutable/Range.class",
    "scala/collection/immutable/Set.class",
    "scala/collection/immutable/Set$.class",
    "scala/collection/immutable/SortedSet.class",
    "scala/collection/immutable/SortedSet$.class",
    "scala/collection/immutable/TreeSet.class",
    "scala/collection/immutable/TreeSet$.class",
    "scala/collection/immutable/SortedMap.class",
    "scala/collection/immutable/SortedMap$.class",
    "scala/collection/immutable/TreeMap.class",
    "scala/collection/immutable/TreeMap$.class",
    "scala/collection/immutable/BitSet.class",
    "scala/collection/immutable/BitSet$.class",
    "scala/collection/immutable/Seq.class",
    "scala/collection/immutable/Seq$.class",
    "scala/collection/immutable/LazyList.class",
    "scala/collection/immutable/LazyList$.class",
    "scala/runtime/RichFloat.class",
    "scala/runtime/RichByte.class",
    "scala/runtime/RichShort.class",
    "scala/runtime/RichBoolean.class",
    "scala/collection/mutable/ArrayBuffer.class",
    "scala/collection/mutable/ArrayBuffer$.class",
    "scala/collection/mutable/ListBuffer.class",
    "scala/collection/mutable/ListBuffer$.class",
    "scala/collection/mutable/ArrayDeque.class",
    "scala/collection/mutable/ArrayDeque$.class",
    "scala/collection/mutable/StringBuilder.class",
    "scala/collection/mutable/StringBuilder$.class",
    "scala/collection/mutable/HashMap.class",
    "scala/collection/mutable/HashMap$.class",
    "scala/collection/mutable/HashSet.class",
    "scala/collection/mutable/HashSet$.class",
    "scala/collection/mutable/LinkedHashMap.class",
    "scala/collection/mutable/LinkedHashMap$.class",
    "scala/collection/mutable/LinkedHashSet.class",
    "scala/collection/mutable/LinkedHashSet$.class",
    "scala/collection/immutable/NumericRange.class",
    "scala/collection/immutable/NumericRange$.class",
    "scala/collection/immutable/NumericRange$Inclusive.class",
    "scala/collection/immutable/NumericRange$Exclusive.class",
    "scala/util/Either.class",
    "scala/util/Left.class",
    "scala/util/Right.class",
    "scala/App.class",
    "scala/DelayedInit.class",
    "scala/util/Left$.class",
    "scala/util/Right$.class",
    "scala/util/Try.class",
    "scala/util/Try$.class",
    "scala/util/Success.class",
    "scala/util/Success$.class",
    "scala/util/Failure.class",
    "scala/util/Failure$.class",
    "scala/util/control/Breaks.class",
    "scala/util/control/Breaks$.class",
    "scala/util/control/Breaks$TryBlock.class",
    "scala/util/control/Breaks$$anon$1.class",
    "scala/math/BigInt.class",
    "scala/math/BigInt$.class",
    "scala/math/BigDecimal.class",
    "scala/math/BigDecimal$.class",
    "scala/util/ChainingOps.class",
    "scala/util/ChainingOps$.class",
    "scala/util/package$chaining$.class",
    "scala/collection/View.class",
    "scala/collection/View$.class",
    "scala/collection/SeqView.class",
    "scala/util/Using.class",
    "scala/util/Using$.class",
    "scala/util/Using$Manager.class",
    "scala/util/Using$Manager$.class",
    "scala/util/Using$Releasable.class",
    "scala/util/Using$Releasable$.class",
    "scala/util/Using$Releasable$AutoCloseableIsReleasable$.class",
    "scala/util/ChainingSyntax.class",
    "scala/runtime/IntRef.class",
    "scala/runtime/ObjectRef.class",
    "scala/runtime/LongRef.class",
    "scala/runtime/BooleanRef.class",
    "scala/util/matching/Regex.class",
    "scala/Array$.class",
    "scala/runtime/NonLocalReturnControl.class",
];

fn assert_no_private_stdlib(out: &Path) {
    for rel in LIBRARY_COLLIDERS {
        let p = out.join(rel);
        assert!(
            !p.is_file(),
            "library ABI must not emit {} (would collide with scala-library.jar)",
            p.display()
        );
    }
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn scala_xml_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-xml_2.13-2.3.0.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/modules/scala-xml_2.13/2.3.0/scala-xml_2.13-2.3.0.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_no_private_stdlib(&out);
    let mut cp = format!("{}:{}", out.display(), jar.display());
    if let Some(xml) = scala_xml_jar() {
        cp.push(':');
        cp.push_str(&xml.display().to_string());
    }
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp out:scala-library failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    // Official ~20MB 2.13.16 distribution. Skip if curl/tar is unavailable.
    let tgz = PathBuf::from("/tmp/scala-2.13.16.tgz");
    let url = "https://github.com/scala/scala/releases/download/v2.13.16/scala-2.13.16.tgz";
    let status = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "30",
            "-o",
            tgz.to_str().unwrap(),
            url,
        ])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && tgz.is_file() {
        let _ = Command::new("tar")
            .args(["-xzf", tgz.to_str().unwrap(), "-C", "/tmp"])
            .status();
        if cached.is_file() {
            return Some(cached);
        }
    }
    None
}

/// The fixture's stdout must match what real scalac 2.13.16 produces for the
/// same source, not just our recorded expectation.
fn namedargs_scalac_dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip real-scalac diff {name}: scalac not obtainable");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip real-scalac diff {name}: scala-library jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let ref_cp = format!("{}:{}", ref_out.display(), jar.display());
    let reference = Command::new("java")
        .args(["-cp", &ref_cp, "Main"])
        .output()
        .expect("java (real scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed for {name}: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let reference = String::from_utf8_lossy(&reference.stdout).to_string();
    assert_eq!(
        reference,
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );

    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    let cp = format!("{}:{}", out.display(), jar.display());
    let ours = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java (our build)");
    assert!(
        ours.status.success(),
        "java -Xverify:all Main (our build) failed for {name}: {}",
        String::from_utf8_lossy(&ours.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&ours.stdout),
        reference,
        "stdout differs from real scalac for {name}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// Alias type members (`Later.Scope`, `Cfg.Names`), a factory `apply` on a
/// plain class's companion, an overload whose alternatives differ by a
/// trailing implicit clause after a repeated parameter, and a `val` that wins
/// over the inherited `Any.==` in value and in extractor position.
/// Library ABI only: the private runtime has no `Seq` for a repeated parameter.
#[test]
fn scala_library_dual_run_ovl() {
    dual_run_fixture("ovl");
}

/// `Lit(1, 2)` matches `(Int, Any)` and `(Any, Int)` equally well; scalac
/// reports `ambiguous reference to overloaded definition`.
#[test]
fn fixtures_ovl_ambiguous_bad_is_error() {
    compile_fails_lib("ovl_ambiguous_bad", "ambiguous overload for apply");
}

/// A default on the last parameter does not make the earlier ones optional,
/// and the companion's own `apply` is still checked against its parameter
/// types. scalac reports a type mismatch on the first and third argument.
#[test]
fn fixtures_ovl_none_bad_is_error() {
    compile_fails_lib("ovl_none_bad", "no matching overload");
}

/// nsc's `instantiateExpecting`: the expected type constrains a method's type
/// parameters as much as the arguments do. `Array("x", "y")` checked against
/// `Array[AnyRef]` is an `Array[AnyRef]` (`Array` is invariant), and
/// `Library.column("id"): Rep[Int]` knows `T = Int` before its
/// `implicit TypedType[T]` is searched for. A covariant occurrence stays a
/// mere upper bound, so `cov("q"): List[Any]` keeps `T = String`.
#[test]
fn scala_library_dual_run_exptype() {
    dual_run_fixture("exptype");
}

/// The same fixture compiled by the real scalac 2.13.16: the recorded
/// expectation, scalac's stdout and ours all have to agree, because the point
/// of the fixture is *which* type argument gets inferred.
#[test]
fn real_scalac_dual_run_exptype() {
    namedargs_scalac_dual_run("exptype");
}

/// Neither the arguments nor the expected type pin `T` down: the implicit is
/// searched with `T` still open and nothing is silently filled in.
#[test]
fn fixtures_exptype_unsolved_bad_is_error() {
    compile_fails_lib(
        "exptype_unsolved_bad",
        "could not find implicit value of type TypedType[T]",
    );
}

/// `Array` is invariant: an `Array[Int]` is not an `Array[Any]`, neither in an
/// ascription nor as an argument.
#[test]
fn fixtures_exptype_arrayvar_bad_is_error() {
    compile_fails_lib(
        "exptype_arrayvar_bad",
        "type mismatch; found: Array[Int]  required: Array[Any]",
    );
}

/// Separate compilation of the three shapes whose classfiles we used not to
/// write at all: a package object's mirror class, a value class's companion,
/// and an operator-named nested object.
///
/// `mcls_main.scala` is compiled against `mcls_lib.scala`'s **classfiles**,
/// never alongside its source, so a member that only exists inside one run
/// fails here.
#[test]
fn separate_compilation_package_object_value_class_and_operator_name() {
    if !java_available() {
        return;
    }
    let out_lib = tmp_dir("mcls-lib");
    let out_main = tmp_dir("mcls-main");
    let jar = scala_library_jar();
    let compile = |src: PathBuf, out: &Path, cp: Option<&Path>| {
        let mut cmd = Command::new(bin());
        cmd.args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ]);
        match &jar {
            Some(j) => {
                cmd.args(["--scala-library", j.to_str().unwrap()]);
            }
            None => {
                cmd.arg("--no-scala-library");
            }
        }
        if let Some(p) = cp {
            cmd.args(["-cp", p.to_str().unwrap()]);
        }
        cmd.status().expect("scala-rs compile")
    };
    assert!(compile(fixtures_dir().join("mcls_lib.scala"), &out_lib, None).success());
    // The exact set scalac 2.13.16 writes for this source.
    for f in [
        // A package object is two classfiles; the *mirror* is where nsc puts
        // the pickle, and without it a consumer sees no member at all.
        "mcls/util/package$.class",
        "mcls/util/package.class",
        // A value class's `$extension` methods are declared on its companion.
        "mcls/Meters$.class",
        "mcls/Wrap$.class",
        // Operator characters are `NameTransformer`-encoded in a class name
        // too: not `Codes$:@$.class`.
        "mcls/Codes$$colon$at$.class",
    ] {
        assert!(
            out_lib.join(f).is_file(),
            "{f} missing in {}",
            out_lib.display()
        );
    }
    assert!(compile(
        fixtures_dir().join("mcls_main.scala"),
        &out_main,
        Some(&out_lib)
    )
    .success());
    let mut cp = format!("{}:{}", out_main.display(), out_lib.display());
    if let Some(j) = &jar {
        cp = format!("{}:{}", cp, j.display());
    }
    let output = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("mcls_main")
    );
    let _ = fs::remove_dir_all(&out_lib);
    let _ = fs::remove_dir_all(&out_main);
}

/// The same library, with **real scalac** as the consumer -- the only check
/// that our pickle says what nsc needs rather than only what we read back.
///
/// Every one of these failed before: the package object's members were
/// invisible (`object twice is not a member of package mcls.util`), and the
/// value class crashed scalac's own erasure phase with `AssertionError: no
/// extension method found` and then, once the companion existed, with
/// `unexpected type representation ... <notype>` until the parameter accessor
/// carried `PARAMACCESSOR`.
#[test]
fn scalac_uses_our_package_object_and_value_class() {
    let Some(scalac) = find_scalac() else {
        eprintln!("scalac 2.13 not obtainable; skipping (documented in README)");
        return;
    };
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        return;
    };
    let out_lib = tmp_dir("mcls-scalac-lib");
    let out_main = tmp_dir("mcls-scalac-main");
    let status = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("mcls_lib.scala").to_str().unwrap(),
            "-d",
            out_lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("scala-rs compile mcls_lib");
    assert!(status.success());
    let out = Command::new(&scalac)
        .args([
            "-d",
            out_main.to_str().unwrap(),
            "-classpath",
            out_lib.to_str().unwrap(),
            fixtures_dir().join("mcls_main.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "scalac could not compile against our classfiles:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let cp = format!(
        "{}:{}:{}",
        out_main.display(),
        out_lib.display(),
        jar.display()
    );
    let run = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java Main (scalac-compiled) failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected_stdout("mcls_main")
    );
    let _ = fs::remove_dir_all(&out_lib);
    let _ = fs::remove_dir_all(&out_main);
}
