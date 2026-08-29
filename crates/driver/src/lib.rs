//! Compiler driver: parse → namer → typer → uncurry → lambda-lift → erasure → emit → write class files.

use std::path::{Path, PathBuf};
use std::process::Command;

use scala_rs_backend::{emit_opts, emit_runtime, load_classpath, EmitOpts};
use scala_rs_parser::{dump_tree, parse_file_opts, ParseOptions, Tree};
use scala_rs_span::{render_all, Diagnostic, Level, SourceFile, Span};
use scala_rs_typer::{
    erase, find_mains, lambda_lift, mark_anon_captures, note_source_value_classes, typecheck_units,
    uncurry, ClasspathClass, ClasspathMethod, ClasspathPickleMethod, ClasspathType,
    ClasspathTypeParam, TypecheckOptions,
};

pub use scala_rs_backend::EmittedClass;

/// Options for [`compile_paths`].
#[derive(Clone, Debug)]
pub struct CompileOptions {
    /// Directory class files are written to.
    pub out_dir: PathBuf,
    /// `--parse`: dump the AST and skip typechecking / emit.
    pub parse_only: bool,
    /// `--typer`: dump the typed tree after typechecking.
    pub typer_dump: bool,
    /// `-Xfatal-warnings`: promote warnings (e.g. non-exhaustive match) to errors.
    pub fatal_warnings: bool,
    /// When set, bytecode targets scala-library 2.13 on this jar (do not emit
    /// private Option/List/FunctionN stand-ins). The path is also added to the
    /// `java -cp` of [`run_main`] callers that pass it through.
    pub scala_library: Option<PathBuf>,
    /// Directories (or class files) searched for previously compiled classes.
    pub class_path: Vec<PathBuf>,
    /// `-language:feat` flags (`postfixOps`, `implicitConversions`, `dynamics`).
    pub language_features: Vec<String>,
    /// `-Xsource:3` / `-Xsource:3-cross`: accept the Scala 3 spellings this
    /// subset implements (`A & B` compound types).
    pub xsource3: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions {
            out_dir: PathBuf::from("."),
            parse_only: false,
            typer_dump: false,
            fatal_warnings: false,
            scala_library: None,
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
        }
    }
}

/// Result of compiling one or more source files.
pub struct CompileResult {
    pub diags: Vec<Diagnostic>,
    pub sources: Vec<SourceFile>,
    pub emitted: Vec<EmittedClass>,
    /// Simple object names that define `main`, e.g. `"Main"`.
    pub mains: Vec<String>,
}

impl CompileResult {
    /// True when there are no error-level diagnostics.
    pub fn ok(&self) -> bool {
        !has_errors(&self.diags)
    }

    /// Render all diagnostics against the collected source files.
    pub fn render_diags(&self) -> String {
        render_all(&self.diags, &self.sources)
    }
}

struct Unit {
    file_index: usize,
    tree: Tree,
    pickles: std::collections::HashMap<u32, Vec<u8>>,
}

fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.level == Level::Error)
}

fn source_file_name(sf: &SourceFile) -> &str {
    sf.path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(sf.name.as_str())
}

fn dump_unit(source: &SourceFile, tree: &Tree) {
    println!("// {}", source.name);
    let dump = dump_tree(tree);
    print!("{dump}");
    if !dump.ends_with('\n') {
        println!();
    }
}

fn failed_result(diags: Vec<Diagnostic>, sources: Vec<SourceFile>) -> CompileResult {
    CompileResult {
        diags,
        sources,
        emitted: Vec::new(),
        mains: Vec::new(),
    }
}

/// Load, parse, typecheck, and emit each path.
///
/// Files are parsed first. Any parse/load error stops the pipeline (no typer,
/// no emit). Typechecking is sequential and per-file for v1 (namers do not see
/// other compilation units). Class files are written to `opts.out_dir` on
/// success unless `parse_only` is set.
pub fn compile_paths(files: &[PathBuf], opts: &CompileOptions) -> CompileResult {
    let mut diags = Vec::new();
    let mut sources = Vec::new();
    let mut units = Vec::new();

    for path in files {
        let file_index = sources.len();
        match SourceFile::load(path) {
            Ok(sf) => {
                let parsed = parse_file_opts(
                    &sf,
                    file_index,
                    ParseOptions {
                        source3: opts.xsource3,
                    },
                );
                diags.extend(parsed.diags);
                units.push(Unit {
                    file_index,
                    tree: parsed.tree,
                    pickles: std::collections::HashMap::new(),
                });
                sources.push(sf);
            }
            Err(e) => {
                let name = path.display().to_string();
                diags.push(Diagnostic::error(
                    file_index,
                    Span::DUMMY,
                    format!("cannot read {}: {e}", path.display()),
                ));
                sources.push(SourceFile::from_path(path.clone(), name, String::new()));
            }
        }
    }

    if has_errors(&diags) {
        return failed_result(diags, sources);
    }

    if opts.parse_only {
        for u in &units {
            dump_unit(&sources[u.file_index], &u.tree);
        }
        return CompileResult {
            diags,
            sources,
            emitted: Vec::new(),
            mains: Vec::new(),
        };
    }

    let mut mains = Vec::new();
    let shared_st;
    {
        // One symbol table for the whole run: every unit is named before any
        // is typed, so files can reference each other.
        let mut refs: Vec<(&mut Tree, usize)> = units
            .iter_mut()
            .map(|u| {
                let fi = u.file_index;
                (&mut u.tree, fi)
            })
            .collect();
        let (mut st, tdiags) = typecheck_units(
            &mut refs,
            &TypecheckOptions {
                fatal_warnings: opts.fatal_warnings,
                library_abi: opts.scala_library.is_some(),
                classpath: load_cp(&opts.class_path),
                binary_path: {
                    let mut p = opts.class_path.clone();
                    if let Some(j) = &opts.scala_library {
                        p.push(j.clone());
                    }
                    p
                },
                language_features: opts.language_features.clone(),
            },
        );
        diags.extend(tdiags);
        for u in units.iter() {
            mains.extend(find_mains(&st, &u.tree));
        }
        if !has_errors(&diags) {
            for u in units.iter_mut() {
                uncurry(&mut u.tree, &mut st);
                lambda_lift(&mut u.tree, &mut st);
                mark_anon_captures(&u.tree, &mut st);
            }
            let pickles = scala_rs_backend::pickle::pickle_all(&st);
            // Value classes are boxed across unit boundaries, so every unit's
            // declarations have to be known before the first one is erased.
            for u in units.iter() {
                note_source_value_classes(&u.tree, &mut st);
            }
            for u in units.iter_mut() {
                u.pickles = pickles.clone();
                erase(&mut u.tree, &mut st);
            }
        }
        shared_st = Some(st);
    }

    if opts.typer_dump {
        for u in &units {
            dump_unit(&sources[u.file_index], &u.tree);
        }
    }

    if has_errors(&diags) {
        return CompileResult {
            diags,
            sources,
            emitted: Vec::new(),
            mains,
        };
    }

    let library_abi = opts.scala_library.is_some();
    let mut emitted = if library_abi {
        Vec::new()
    } else {
        emit_runtime()
    };
    let st = shared_st.as_ref().expect("the run is typed");
    // A class can mix in a trait defined in another file, so the concrete
    // trait members of the whole run have to be known before emitting any.
    let mut trait_members = scala_rs_backend::gen::TraitImpls::default();
    for u in &units {
        scala_rs_backend::gen::collect_trait_members(&u.tree, st, &mut trait_members);
    }
    for u in &units {
        let src_name = source_file_name(&sources[u.file_index]);
        emitted.extend(emit_opts(
            &u.tree,
            st,
            src_name,
            EmitOpts {
                library_abi,
                pickles: u.pickles.clone(),
                trait_members: Some(trait_members.clone()),
            },
        ));
    }

    if let Err(e) = write_emitted(&emitted, &opts.out_dir) {
        diags.push(Diagnostic::error(
            0,
            Span::DUMMY,
            format!(
                "cannot write class files to {}: {e}",
                opts.out_dir.display()
            ),
        ));
    }

    CompileResult {
        diags,
        sources,
        emitted,
        mains,
    }
}

/// Write each class to `out_dir/{internal_name}.class`, creating package
/// subdirectories as needed (`foo/Bar` → `out_dir/foo/Bar.class`).
pub fn write_emitted(emitted: &[EmittedClass], out_dir: &Path) -> std::io::Result<()> {
    if emitted.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(out_dir)?;
    for c in emitted {
        let dest = class_path(out_dir, &c.internal_name);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &c.bytes)?;
    }
    Ok(())
}

fn class_path(out_dir: &Path, internal_name: &str) -> PathBuf {
    let mut dest = out_dir.to_path_buf();
    let parts: Vec<&str> = internal_name.split('/').filter(|p| !p.is_empty()).collect();
    match parts.split_last() {
        Some((file, dirs)) => {
            for d in dirs {
                dest.push(d);
            }
            dest.push(format!("{file}.class"));
        }
        None => dest.push(".class"),
    }
    dest
}

fn cp_type(t: &scala_rs_backend::PickledType) -> ClasspathType {
    ClasspathType {
        name: t.name.clone(),
        args: t.args.iter().map(cp_type).collect(),
    }
}

fn cp_tparam(t: &scala_rs_backend::PickledTypeParam) -> ClasspathTypeParam {
    ClasspathTypeParam {
        name: t.name.clone(),
        tparams: t.tparams.iter().map(cp_tparam).collect(),
    }
}

fn load_cp(paths: &[PathBuf]) -> Vec<ClasspathClass> {
    if paths.is_empty() {
        return Vec::new();
    }
    load_classpath(paths)
        .into_iter()
        .map(|c| {
            let pickle_tparams = c
                .pickle
                .as_ref()
                .map(|p| p.tparams.iter().map(cp_tparam).collect())
                .unwrap_or_default();
            ClasspathClass {
                jvm_name: c.internal_name,
                is_module: c.is_module,
                methods: c
                    .methods
                    .into_iter()
                    .map(|m| ClasspathMethod {
                        name: m.name,
                        desc: m.desc,
                    })
                    .collect(),
                pickle: c.pickle.map(|p| {
                    p.methods
                        .into_iter()
                        .map(|m| ClasspathPickleMethod {
                            name: m.name,
                            param_names: m.param_names,
                            param_types: m.param_types.iter().map(cp_type).collect(),
                            ret: cp_type(&m.ret),
                            tparams: m.tparams.iter().map(cp_tparam).collect(),
                            is_val: m.is_val,
                            is_ctor: m.is_ctor,
                            is_implicit: m.is_implicit,
                        })
                        .collect()
                }),
                pickle_tparams,
                is_interface: c.is_interface,
                super_name: c.super_name,
                interfaces: c.interfaces,
            }
        })
        .collect()
}

/// Run `java -cp out_dir[:extra...] main_class args...`.
///
/// `main_class` may be an internal name (`foo/Bar`); slashes are converted to
/// dots for the JVM (`foo.Bar`).
pub fn run_main(
    out_dir: &Path,
    main_class: &str,
    args: &[String],
) -> std::io::Result<std::process::Output> {
    run_main_with_cp(out_dir, &[], main_class, args)
}

/// Like [`run_main`], with extra classpath entries (e.g. scala-library.jar).
pub fn run_main_with_cp(
    out_dir: &Path,
    extra_cp: &[PathBuf],
    main_class: &str,
    args: &[String],
) -> std::io::Result<std::process::Output> {
    let dotted = main_class.replace('/', ".");
    Command::new("java")
        .arg("-cp")
        .arg(java_classpath(out_dir, extra_cp))
        .arg(&dotted)
        .args(args)
        .output()
}

fn java_classpath(out_dir: &Path, extra_cp: &[PathBuf]) -> String {
    let mut cp = out_dir.as_os_str().to_string_lossy().into_owned();
    for p in extra_cp {
        cp.push(':');
        cp.push_str(&p.to_string_lossy());
    }
    cp
}

/// Locate a scala-library 2.13 jar: `SCALA_LIBRARY_JAR`, then well-known paths
/// (`/tmp/scala-rs-lib/…`, cwd, `lib/`). Does not enable library ABI by itself.
pub fn find_scala_library() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SCALA_LIBRARY_JAR") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut cands = vec![
        PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar"),
        PathBuf::from("scala-library-2.13.16.jar"),
        PathBuf::from("lib/scala-library-2.13.16.jar"),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        cands.push(cwd.join("scala-library-2.13.16.jar"));
        cands.push(cwd.join("lib").join("scala-library-2.13.16.jar"));
        if let Ok(rd) = std::fs::read_dir(&cwd) {
            for e in rd.flatten() {
                let s = e.file_name();
                let s = s.to_string_lossy();
                if s.starts_with("scala-library-2.13") && s.ends_with(".jar") && e.path().is_file()
                {
                    return Some(e.path());
                }
            }
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

/// Locate a scala-xml 2.13 jar: `SCALA_XML_JAR`, then `/tmp/scala-rs-lib`, cwd, `lib/`.
pub fn find_scala_xml() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SCALA_XML_JAR") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut cands = vec![
        PathBuf::from("/tmp/scala-rs-lib/scala-xml_2.13-2.3.0.jar"),
        PathBuf::from("scala-xml_2.13-2.3.0.jar"),
        PathBuf::from("lib/scala-xml_2.13-2.3.0.jar"),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        cands.push(cwd.join("scala-xml_2.13-2.3.0.jar"));
        cands.push(cwd.join("lib").join("scala-xml_2.13-2.3.0.jar"));
        if let Ok(rd) = std::fs::read_dir(&cwd) {
            for e in rd.flatten() {
                let s = e.file_name();
                let s = s.to_string_lossy();
                if s.starts_with("scala-xml_2.13") && s.ends_with(".jar") && e.path().is_file() {
                    return Some(e.path());
                }
            }
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fresh_dir() -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "scala-rs-driver-{}-{}-{}",
            std::process::id(),
            n,
            nanos
        ));
        std::fs::create_dir_all(&p).expect("create temp dir");
        TempDir(p)
    }

    fn java_available() -> bool {
        Command::new("java")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn write_emitted_package_and_module_names() {
        let tmp = fresh_dir();
        let emitted = vec![
            EmittedClass {
                internal_name: "Main$".into(),
                bytes: vec![0xCA, 0xFE],
            },
            EmittedClass {
                internal_name: "foo/Bar".into(),
                bytes: vec![0xBA, 0xBE],
            },
        ];
        write_emitted(&emitted, &tmp.0).unwrap();
        let module = tmp.0.join("Main$.class");
        let pkg = tmp.0.join("foo").join("Bar.class");
        assert!(module.is_file(), "missing {}", module.display());
        assert!(pkg.is_file(), "missing {}", pkg.display());
        assert_eq!(std::fs::read(module).unwrap(), vec![0xCA, 0xFE]);
        assert_eq!(std::fs::read(pkg).unwrap(), vec![0xBA, 0xBE]);
    }

    #[test]
    fn compile_result_ok_is_false_on_errors() {
        let r = CompileResult {
            diags: vec![Diagnostic::error(0, Span::DUMMY, "boom")],
            sources: vec![],
            emitted: vec![],
            mains: vec![],
        };
        assert!(!r.ok());
        let r = CompileResult {
            diags: vec![],
            sources: vec![],
            emitted: vec![],
            mains: vec![],
        };
        assert!(r.ok());
    }

    #[test]
    fn compile_hello_snippet_and_maybe_run() {
        let tmp = fresh_dir();
        let src = tmp.0.join("Hello.scala");
        std::fs::write(
            &src,
            r#"
object Main {
  def main(args: Array[String]): Unit = println("hello, scala-rs")
}
"#,
        )
        .unwrap();

        let opts = CompileOptions {
            out_dir: tmp.0.clone(),
            parse_only: false,
            typer_dump: false,
            fatal_warnings: false,
            scala_library: None,
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
        };
        let result = compile_paths(&[src], &opts);
        assert!(result.ok(), "compile failed:\n{}", result.render_diags());
        assert!(
            result.mains.iter().any(|m| m == "Main"),
            "expected Main in {:?}",
            result.mains
        );

        if java_available() && !result.emitted.is_empty() {
            let main = result.mains.first().map(String::as_str).unwrap_or("Main");
            let output = run_main(&tmp.0, main, &[]).expect("run java");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success(),
                "java failed: status={:?} stdout={stdout:?} stderr={stderr:?}",
                output.status
            );
            assert!(
                stdout.contains("hello, scala-rs"),
                "unexpected stdout: {stdout:?}"
            );
        }
    }

    #[test]
    fn parse_only_does_not_emit() {
        let tmp = fresh_dir();
        let src = tmp.0.join("P.scala");
        std::fs::write(
            &src,
            "object Main { def main(args: Array[String]): Unit = () }\n",
        )
        .unwrap();
        let opts = CompileOptions {
            out_dir: tmp.0.join("out"),
            parse_only: true,
            typer_dump: false,
            fatal_warnings: false,
            scala_library: None,
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
        };
        let result = compile_paths(&[src], &opts);
        assert!(result.ok(), "{}", result.render_diags());
        assert!(result.emitted.is_empty());
        assert!(!opts.out_dir.exists());
    }

    #[test]
    fn type_error_is_not_ok() {
        let tmp = fresh_dir();
        let src = tmp.0.join("Bad.scala");
        std::fs::write(&src, "object M { def f(): Int = foo }\n").unwrap();
        let opts = CompileOptions {
            out_dir: tmp.0.join("out"),
            parse_only: false,
            typer_dump: false,
            fatal_warnings: false,
            scala_library: None,
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
        };
        let result = compile_paths(&[src], &opts);
        assert!(!result.ok());
        assert!(result.emitted.is_empty());
        assert!(result.diags.iter().any(|d| d.message.contains("not found")));
    }

    #[test]
    fn library_abi_skips_private_runtime() {
        let tmp = fresh_dir();
        let src = tmp.0.join("Lib.scala");
        std::fs::write(
            &src,
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 1 :: Nil
    println(Some(xs).isEmpty)
  }
}
"#,
        )
        .unwrap();
        let opts = CompileOptions {
            out_dir: tmp.0.join("out"),
            parse_only: false,
            typer_dump: false,
            fatal_warnings: false,
            scala_library: Some(PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar")),
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
        };
        let result = compile_paths(&[src], &opts);
        assert!(result.ok(), "compile failed:\n{}", result.render_diags());
        assert!(
            result
                .emitted
                .iter()
                .all(|c| !c.internal_name.starts_with("scala/")),
            "emitted {:?}",
            result
                .emitted
                .iter()
                .map(|c| c.internal_name.as_str())
                .collect::<Vec<_>>()
        );
        assert!(result.emitted.iter().any(|c| c.internal_name == "Main$"));
    }
}
