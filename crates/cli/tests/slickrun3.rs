//! E2E tests for the `agent/slickrun3` slice (fixture prefix `slickrun3`).
//!
//! `tests/slick_run.sh`'s `p12_mapped` died on the very first line with
//!
//! ```text
//! NoSuchMethodError: slick.ast.TypedType
//!   RelationalTypesComponent$MappedColumnTypeFactory.base(
//!     Function1, Function1, ClassTag, slick.ast.TypedType)
//! ```
//!
//! Removing that took five defects, and the fifth left slick's whole query
//! compiler and SQL generator running on scala-rs-compiled class files,
//! producing SQL byte-identical to real scalac's build. Each one is a case in
//! `tests/fixtures/slickrun3.scala`:
//!
//! 1. **An abstract type member with a compound upper bound erased to
//!    `Object`.** SLS 3.7 erases it like a type parameter, and nsc reduces a
//!    compound bound with `Erasure.intersectionDominator` -- a parent that
//!    some other parent extends is shadowed, a real class beats a trait, and
//!    otherwise the first one wins. slick's `type BaseColumnType[T] <:
//!    ColumnType[T] & BaseTypedType[T]` is `slick.ast.TypedType`.
//!
//! 2. **A subclass that narrows such a parameter got no bridge.** After
//!    erasure nothing says `base(…, TypedType)` and `base(…, JdbcType)` are
//!    one method rather than two overloads, so `SymbolTable::
//!    erased_abstract_params` records which parameters were abstract *before*
//!    erasure and `gen::bridge_overrides` reads it. Without the bridge the
//!    interface method stayed abstract (`AbstractMethodError`).
//!
//! 3. **`intersectionDominator` drops information, and the call site pays for
//!    it.** scala-reflect's `type TermName >: Null <: TermNameApi with Name`
//!    erases to the *interface* `Names$TermNameApi` -- nsc's own `newTermName`
//!    returns exactly that -- which does not extend the abstract *class*
//!    `Names$NameApi`; only `Name` brings that in. Passing one where the other
//!    is expected needs a `checkcast` (`VerifyError: Type Names$TermNameApi is
//!    not assignable to Names$NameApi`, in the `ShapedValue` macro's
//!    expansion).
//!
//! 4. **A local's type was re-read through `this`.** `Typer::bind_found`
//!    applied `expand_type_members` to every identifier, so a local whose type
//!    mentioned `map.Self` was rebound to the enclosing class's `Self`. Only a
//!    *member* of a class is seen through a prefix. slick's
//!    `ResultSetMapping.withInferredType` destructured `(map2, newType)` and
//!    `checkcast`ed a plain `Node` to a `ResultSetMapping` -- every query the
//!    compiler ran.
//!
//! 5. **A block in statement position asked for its value.** `gen_stat` had no
//!    `Block` arm, so it fell through to `gen_expr` + pop and put a branching
//!    last expression back in value mode. slick's
//!    `QueryInterpolator.appendString` then generated an inner `match` for its
//!    `Any` lub and only the arms whose own type was not `Unit` left anything
//!    on the stack (`VerifyError: Inconsistent stackmap frames`).
//!
//! 6. **`withFilter`'s result was replaced by the receiver's type.** That rule
//!    exists for the collections, where the declared result is the receiver
//!    *widened*; it is wrong for a `withFilter` returning something else.
//!    slick's `ConstArray.withFilter(p): ConstArrayOp[T]` made the following
//!    `foreach` resolve to `ConstArray`'s and `checkcast` the anonymous
//!    `ConstArrayOp` to a `ConstArray`.
//!
//! 7. **`super.m` landed on a mixin that only re-declares `m`.** nsc resolves
//!    it to the first *concrete* member along the linearization. slick's
//!    `BasicStreamingQueryActionExtensionMethodsImpl` narrows `result`
//!    covariantly and leaves it abstract, and the `$class` holder the emitted
//!    `invokestatic` named does not exist at all
//!    (`NoClassDefFoundError`).
//!
//! Self-contained (own copies of the small helpers `e2e.rs` also has) per
//! `.agent-brief.md`: the shared test files belong to other in-flight agents.

use std::fs;
use std::path::PathBuf;
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
        "scala-rs-slickrun3-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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

/// Compile the fixture against the real scala-library; returns the output dir.
fn compile_fixture() -> Option<PathBuf> {
    let jar = scala_library_jar()?;
    let out = tmp_dir("out");
    let res = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("slickrun3.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        res.status.success(),
        "scala-rs failed on slickrun3.scala:\n{}\n{}",
        String::from_utf8_lossy(&res.stdout),
        String::from_utf8_lossy(&res.stderr)
    );
    Some(out)
}

/// The whole fixture, run under the bytecode verifier, against real scalac
/// 2.13.16's own stdout for the same file.
#[test]
fn fixtures_slickrun3_matches_scalac() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip slickrun3: no scala-library jar");
        return;
    };
    let out = compile_fixture().unwrap();
    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "Main",
        ])
        .output()
        .expect("run the fixture");
    assert!(
        run.status.success(),
        "slickrun3 failed at run time:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join("slickrun3.txt")).unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = fs::remove_dir_all(&out);
}

fn javap(out: &PathBuf, class: &str) -> Option<String> {
    let res = Command::new("javap")
        .args(["-p", "-cp", out.to_str().unwrap(), class])
        .output()
        .ok()?;
    res.status
        .success()
        .then(|| String::from_utf8_lossy(&res.stdout).into_owned())
}

/// The descriptors themselves, so a change that keeps the stdout by some other
/// route still has to say so out loud. Every expectation here is what real
/// scalac 2.13.16 emits for the same fixture.
#[test]
fn fixtures_slickrun3_erases_compound_bounds_like_nsc() {
    let Some(out) = compile_fixture() else {
        eprintln!("skip slickrun3 javap: no scala-library jar");
        return;
    };
    let Some(factory) = javap(&out, "TypesComponent$ColumnTypeFactory") else {
        eprintln!("skip slickrun3 javap: no javap");
        return;
    };
    // `BaseColumnType[T] <: ColumnType[T] with BaseTypedType[T]` erases to the
    // dominator `ColumnType`, hence to *its* bound `TypedType` -- not to
    // `Object`, and not to the shadowed-out `BaseTypedType`.
    assert!(
        factory.contains("TypedType base(java.lang.String, TypedType)"),
        "the interface should declare the wide `base`:\n{factory}"
    );
    assert!(
        factory.contains("void assertNonNull(TypedType)"),
        "and `assertNonNull` at the same erasure:\n{factory}"
    );

    let mapped = javap(&out, "JdbcComponent$MappedJdbcType$").unwrap();
    assert!(
        mapped.contains("JdbcType base(java.lang.String, JdbcType)"),
        "the narrow implementation:\n{mapped}"
    );
    assert!(
        mapped.contains("TypedType base(java.lang.String, TypedType)"),
        "and the bridge the interface call needs:\n{mapped}"
    );

    // The other half of a compound bound really is dropped: `TermName >: Null
    // <: TermNameApi with Name` is `TermNameApi`, exactly as nsc's own
    // `Names.newTermName` is declared in scala-reflect.jar.
    let universe = javap(&out, "Universe$").unwrap();
    assert!(
        universe.contains("TermNameApi mkTerm(java.lang.String)"),
        "`mkTerm`'s bridge should be at `TermNameApi`:\n{universe}"
    );
    let _ = fs::remove_dir_all(&out);
}
