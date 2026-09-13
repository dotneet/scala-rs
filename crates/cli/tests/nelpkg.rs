//! Separate-compilation regression for a polymorphic alias inherited by a
//! package object. Cats' `data.package$` extends a version-specific helper;
//! `NonEmptyLazyList[A]` is declared on that helper while the package also
//! contains the same-named arity-0 newtype module.

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

fn scala_library_jar() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.is_file().then_some(path)
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "scala-rs-nelpkg-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile(
    src: &Path,
    out: &Path,
    cp: Option<&Path>,
    scala_library: Option<&Path>,
) -> std::process::Output {
    let mut cmd = Command::new(bin());
    cmd.args(["compile"]);
    if let Some(jar) = scala_library {
        cmd.args(["--scala-library", jar.to_str().unwrap()]);
    } else {
        cmd.arg("--no-scala-library");
    }
    cmd.arg(src).args(["-d", out.to_str().unwrap()]);
    if let Some(cp) = cp {
        cmd.args(["-cp", cp.to_str().unwrap()]);
    }
    cmd.output().expect("run scala-rs compile")
}

#[test]
fn inherited_package_alias_is_available_through_wildcard_import() {
    let Some(scala_library) = scala_library_jar() else {
        eprintln!("skip inherited package alias regression: scala-library jar unavailable");
        return;
    };
    let dir = tmp_dir("separate");
    let lib_out = dir.join("lib");
    let use_out = dir.join("use");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&use_out).unwrap();

    let lib_src = fixtures_dir().join("nel_pkg_alias_lib.scala");
    let use_src = fixtures_dir().join("nel_pkg_alias_use.scala");
    let lib = compile(&lib_src, &lib_out, None, None);
    assert!(
        lib.status.success(),
        "library compile failed:\n{}{}",
        String::from_utf8_lossy(&lib.stderr),
        String::from_utf8_lossy(&lib.stdout)
    );
    assert!(
        lib_out.join("nelpkg/data/package$.class").is_file(),
        "package object class missing in {}",
        lib_out.display()
    );

    let user = compile(&use_src, &use_out, Some(&lib_out), Some(&scala_library));
    assert!(
        user.status.success(),
        "use-site compile failed:\n{}{}",
        String::from_utf8_lossy(&user.stderr),
        String::from_utf8_lossy(&user.stdout)
    );
    assert!(
        use_out.join("nelpkguse/Main.class").is_file(),
        "use-site class missing in {}",
        use_out.display()
    );

    let _ = fs::remove_dir_all(dir);
}
