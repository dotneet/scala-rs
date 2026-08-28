//! Regression test: the full pickle reader must read every `ScalaSignature`
//! in the real scala-library 2.13.16 jar.
//!
//! The jar is not vendored; when it is missing the test is skipped (the same
//! convention the typer's library-ABI tests use).

use scala_rs_backend::pickle_read::{read_pickle, Entry};
use scala_rs_backend::scala_signature_bytes;
use std::io::Read;

fn jar_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("SCALA_LIBRARY_JAR") {
        return std::path::PathBuf::from(p);
    }
    std::path::PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar")
}

/// A classfile annotated `@ScalaSignature` / `@ScalaLongSignature` must yield a
/// pickle: the descriptor is in the constant pool verbatim, so a byte search is
/// an independent oracle for "this class has a pickle".
fn declares_signature(bytes: &[u8]) -> bool {
    const SHORT: &[u8] = b"Lscala/reflect/ScalaSignature;";
    const LONG: &[u8] = b"Lscala/reflect/ScalaLongSignature;";
    bytes.windows(SHORT.len()).any(|w| w == SHORT) || bytes.windows(LONG.len()).any(|w| w == LONG)
}

struct Stats {
    classes: usize,
    declared: usize,
    read: usize,
    entries: usize,
    /// Tags seen at least once, indexed by tag byte.
    tags: [usize; 256],
    /// `name: reason` for classes that declare a signature we could not read.
    failures: Vec<String>,
}

fn scan(path: &std::path::Path) -> Stats {
    let file = std::fs::File::open(path).expect("open jar");
    let mut zip = zip::ZipArchive::new(file).expect("read jar");
    let mut st = Stats {
        classes: 0,
        declared: 0,
        read: 0,
        entries: 0,
        tags: [0usize; 256],
        failures: Vec::new(),
    };
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).expect("zip entry");
        let name = e.name().to_string();
        if !name.ends_with(".class") {
            continue;
        }
        let mut bytes = Vec::new();
        e.read_to_end(&mut bytes).expect("read class");
        st.classes += 1;
        if !declares_signature(&bytes) {
            continue;
        }
        st.declared += 1;
        let Some(raw) = scala_signature_bytes(&bytes) else {
            st.failures
                .push(format!("{name}: ScalaSignature attribute not extracted"));
            continue;
        };
        match read_pickle(&raw) {
            Ok(p) => {
                st.read += 1;
                st.entries += p.entries.len();
                for &t in &p.entry_tags {
                    st.tags[t as usize] += 1;
                }
            }
            Err(err) => st.failures.push(format!("{name}: {err}")),
        }
    }
    st
}

#[test]
fn reads_every_pickle_in_scala_library() {
    let jar = jar_path();
    if !jar.is_file() {
        eprintln!("skipping: {} not found", jar.display());
        return;
    }
    let st = scan(&jar);
    if !st.failures.is_empty() {
        let shown: Vec<&str> = st.failures.iter().take(40).map(|s| s.as_str()).collect();
        panic!(
            "{} of {} declared pickles could not be read:\n{}",
            st.failures.len(),
            st.declared,
            shown.join("\n")
        );
    }
    assert!(st.classes > 2000, "only {} classfiles seen", st.classes);
    assert!(
        st.declared > 700,
        "only {} classfiles declared a ScalaSignature",
        st.declared
    );
    assert_eq!(st.read, st.declared);
    // The library exercises the interesting corners of the format. Tags the
    // 2.13.16 library never uses (and so cannot be covered here): LITERAL(23),
    // LITERALunit(24), LITERALsymbol(37), ANNOTARGARRAY(44), MODIFIERS(50),
    // and the three tags scalac no longer writes at all —
    // IMPLICITMETHODtpe(22), SUPERtpe(46), DEBRUIJNINDEXtpe(47).
    use scala_rs_backend::pickle_read::tags as t;
    for (tag, what) in [
        (t::CLASSsym, "CLASSsym"),
        (t::MODULEsym, "MODULEsym"),
        (t::VALsym, "VALsym"),
        (t::ALIASsym, "ALIASsym"),
        (t::TYPEsym, "TYPEsym"),
        (t::EXTref, "EXTref"),
        (t::EXTMODCLASSref, "EXTMODCLASSref"),
        (t::THIStpe, "THIStpe"),
        (t::SINGLEtpe, "SINGLEtpe"),
        (t::CONSTANTtpe, "CONSTANTtpe"),
        (t::TYPEREFtpe, "TYPEREFtpe"),
        (t::TYPEBOUNDStpe, "TYPEBOUNDStpe"),
        (t::REFINEDtpe, "REFINEDtpe"),
        (t::CLASSINFOtpe, "CLASSINFOtpe"),
        (t::METHODtpe, "METHODtpe"),
        (t::POLYtpe, "POLYtpe"),
        (t::EXISTENTIALtpe, "EXISTENTIALtpe"),
        (t::ANNOTATEDtpe, "ANNOTATEDtpe"),
        (t::ANNOTINFO, "ANNOTINFO"),
        (t::SYMANNOT, "SYMANNOT"),
        (t::CHILDREN, "CHILDREN"),
        (t::TREE, "TREE"),
        (t::LITERALstring, "LITERALstring"),
        (t::LITERALint, "LITERALint"),
        (t::LITERALclass, "LITERALclass"),
        (t::LITERALenum, "LITERALenum"),
        (t::LITERALlong, "LITERALlong"),
        (t::LITERALdouble, "LITERALdouble"),
        (t::NOtpe, "NOtpe"),
        (t::NOPREFIXtpe, "NOPREFIXtpe"),
        (t::NONEsym, "NONEsym"),
    ] {
        assert!(st.tags[tag as usize] > 0, "no {what} entry in the library");
    }
    eprintln!(
        "read {} pickles ({} entries) from {} classfiles",
        st.read, st.entries, st.classes
    );
}

#[test]
fn list_pickle_has_the_collection_members() {
    let jar = jar_path();
    if !jar.is_file() {
        eprintln!("skipping: {} not found", jar.display());
        return;
    }
    let file = std::fs::File::open(&jar).expect("open jar");
    let mut zip = zip::ZipArchive::new(file).expect("read jar");
    let mut bytes = Vec::new();
    zip.by_name("scala/collection/immutable/List.class")
        .expect("List.class")
        .read_to_end(&mut bytes)
        .expect("read List.class");
    let raw = scala_signature_bytes(&bytes).expect("List has a ScalaSignature");
    let p = read_pickle(&raw).expect("List pickle parses");

    // `List` itself, plus the members scalac pickles directly on it.
    let mut saw_list_class = false;
    let mut saw_map = false;
    for (i, e) in p.entries.iter().enumerate() {
        let Entry::ClassSym { info, .. } = e else {
            continue;
        };
        if p.name(info.name) == Some("List") {
            saw_list_class = true;
            assert_eq!(
                p.sym_full_name(i as u32).as_deref(),
                Some("scala.collection.immutable.List")
            );
        }
    }
    for e in &p.entries {
        if let Entry::ValSym { info, .. } = e {
            if p.name(info.name) == Some("map") {
                saw_map = true;
            }
        }
    }
    assert!(saw_list_class, "List CLASSsym not found");
    assert!(saw_map, "List#map VALsym not found");
}
