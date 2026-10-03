//! Compiler driver: parse → namer → typer → uncurry → lambda-lift → erasure → emit → write class files.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use scala_rs_backend::{emit_opts, emit_runtime, load_classpath_checked, EmitOpts};
use scala_rs_parser::{dump_tree, parse_file_opts, ParseOptions, Tree};
use scala_rs_span::{
    finish_diagnostics, render_all, Diagnostic, Level, SourceFile, Span, WarnSettings,
};
use scala_rs_typer::{
    add_value_class_companions, check_local_case_class_captures, check_local_objects,
    collect_source_classes, defines_local_classes, expand_private_names, expand_trait_private_vals,
    find_mains, hoist_default_receivers, lambda_lift, lazy_locals, mark_anon_captures,
    note_source_value_classes, restore_named_arg_order, restore_rassoc_order, typecheck_units_src,
    uncurry, TypecheckOptions,
};

pub use scala_rs_backend::EmittedClass;
pub use scala_rs_typer::{ParsedFeatures, SourceFeature, SourceFeatures};

/// Share validated archive indexes and decoded binary signatures between
/// sequential compilations. Source state remains local to each run.
pub fn enable_resident_archive_cache() {
    scala_rs_typer::javaclass::enable_resident_archive_cache();
    scala_rs_typer::enable_resident_signature_cache();
}

/// Options for [`compile_paths`].
#[derive(Clone, Debug)]
pub struct CompileOptions {
    /// Directory class files are written to.
    pub out_dir: PathBuf,
    /// `--parse`: dump the AST and skip typechecking / emit.
    pub parse_only: bool,
    /// `--typer`: dump the typed tree after typechecking.
    pub typer_dump: bool,
    /// `-Xfatal-warnings` / `-Werror`: as in nsc, the warnings are reported as
    /// warnings and the run fails with `No warnings can be incurred under
    /// -Werror.` (see `scala_rs_span::finish_diagnostics`).
    pub fatal_warnings: bool,
    /// `-deprecation`: report each deprecation instead of the summary line.
    pub deprecation: bool,
    /// `-feature`: report each feature warning instead of the summary line.
    pub feature: bool,
    /// `-nowarn`: report no warnings at all.
    pub nowarn: bool,
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
    /// `-Xsource-features:<features>`. nsc ignores the setting entirely below
    /// `-Xsource:3`, so the CLI clears this (with a warning) when `xsource3`
    /// is false; `-Xsource:3-cross` fills it with every feature.
    pub source_features: SourceFeatures,
    /// `-Xasync`: enable `scala.async.Async.{async, await}`.
    pub xasync: bool,
    /// `-no-specialization`: nsc's "Ignore @specialize annotations." With it
    /// the parser drops the metadata and the method-owned primitive entries;
    /// without it the implemented method slice is active.
    pub no_specialization: bool,
    /// `-Ykind-projector`: accept kind-projector's `*` placeholder and
    /// `λ` / `Lambda` type lambdas. **Not an nsc flag.** kind-projector is a
    /// compiler plugin, and nsc without it rejects exactly the programs this
    /// rejects with the flag off, so the default has to stay off for the
    /// compatibility claim to mean anything. Scala 3 has a flag of this name
    /// for its own (compatible) version of the syntax, which is where the
    /// spelling comes from.
    pub kind_projector: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions {
            out_dir: PathBuf::from("."),
            parse_only: false,
            typer_dump: false,
            fatal_warnings: false,
            deprecation: false,
            feature: false,
            nowarn: false,
            scala_library: None,
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
            source_features: SourceFeatures::default(),
            xasync: false,
            no_specialization: false,
            kind_projector: false,
        }
    }
}

/// The command line a macro implementation sees through `c.compilerSettings`.
///
/// nsc rebuilds it from the settings the user actually set — `scalac -Xasync
/// -Xsource:3 A.scala` reports
/// `List(-usejavacp, -classpath, …, -d, …, -Xasync, -Xsource:3.0.0)`. That
/// list is not decoration: `scala.async.Async.asyncImpl` starts with
///
/// ```text
/// if (!c.compilerSettings.contains("-Xasync"))
///   c.abort(c.macroApplication.pos,
///     "The async requires the compiler option -Xasync (…)")
/// ```
///
/// so the message a user sees for a missing `-Xasync` comes from the *library*
/// reading this list, not from the compiler. `-usejavacp` is nsc's own and is
/// left out here.
fn compiler_settings(opts: &CompileOptions) -> Vec<String> {
    let mut out = Vec::new();
    if !opts.class_path.is_empty() {
        let joined: Vec<String> = opts
            .class_path
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        out.push("-classpath".to_string());
        out.push(joined.join(":"));
    }
    out.push("-d".to_string());
    out.push(opts.out_dir.display().to_string());
    if opts.fatal_warnings {
        out.push("-Xfatal-warnings".to_string());
    }
    for f in &opts.language_features {
        out.push(format!("-language:{f}"));
    }
    if opts.xasync {
        out.push("-Xasync".to_string());
    }
    if opts.no_specialization {
        out.push("-no-specialization".to_string());
    }
    if opts.kind_projector {
        // Not an nsc setting: nsc would show `-Xplugin:<kind-projector jar>`
        // here. A macro that reads this list should see what was actually set.
        out.push("-Ykind-projector".to_string());
    }
    if opts.xsource3 {
        // nsc unparses the `ScalaVersion` setting, not the spelling given.
        out.push("-Xsource:3.0.0".to_string());
    }
    let names = opts.source_features.names();
    if !names.is_empty() {
        out.push(format!("-Xsource-features:{}", names.join(",")));
    }
    out
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
    /// [`scala_rs_parser::ParseResult::next_node_id`].
    next_node_id: u32,
    pickles: std::rc::Rc<std::collections::HashMap<u32, Vec<u8>>>,
    /// nsc's names for the unit's partial-function classes, read off the
    /// typed tree before lowering reshapes what they are named after.
    anonfun_names: std::rc::Rc<scala_rs_typer::AnonfunClassNames>,
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
    let (java, scala): (Vec<PathBuf>, Vec<PathBuf>) = files
        .iter()
        .cloned()
        .partition(|f| f.extension().is_some_and(|e| e == "java"));
    let mut result = if java.is_empty() {
        compile_paths_unreported(files, opts)
    } else {
        compile_with_java_sources(&java, &scala, opts)
    };
    // nsc's reporting layer: phase order, summaries, `-Werror`.
    result.diags = finish_diagnostics(
        std::mem::take(&mut result.diags),
        &WarnSettings {
            deprecation: opts.deprecation,
            feature: opts.feature,
            nowarn: opts.nowarn,
            fatal_warnings: opts.fatal_warnings,
        },
    );
    result
}

/// A mixed compilation: the Scala sources are typed against the Java ones.
///
/// nsc reads a Java source for its declarations and leaves its class files to
/// javac, which a build tool runs afterwards. This compiler has no Java
/// parser, so javac compiles the Java sources first, into a directory that
/// joins the classpath for this run only and is removed afterwards; their
/// class files are not written to the output directory, as with scalac. That
/// covers Java sources that stand on their own -- annotations, enums,
/// interfaces -- and not ones that refer back to this run's Scala sources,
/// which javac then reports.
fn compile_with_java_sources(
    java: &[PathBuf],
    scala: &[PathBuf],
    opts: &CompileOptions,
) -> CompileResult {
    let error = |message: String| CompileResult {
        diags: vec![Diagnostic::error(0, Span::DUMMY, message)],
        sources: Vec::new(),
        emitted: Vec::new(),
        mains: Vec::new(),
    };
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "scala-rs-java-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return error(format!("cannot create {}: {e}", dir.display()));
    }
    let mut classpath = opts.class_path.clone();
    if let Some(library) = &opts.scala_library {
        classpath.push(library.clone());
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    let joined = classpath
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(sep);
    let javac = std::env::var_os("JAVA_HOME")
        .map(|home| PathBuf::from(home).join("bin").join("javac"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("javac"));
    let mut command = Command::new(&javac);
    command.arg("-proc:none").arg("-nowarn").arg("-d").arg(&dir);
    if !joined.is_empty() {
        command.arg("-cp").arg(&joined);
    }
    let output = command.args(java).output();
    let result = match output {
        Err(e) => error(format!(
            "cannot run {} for the Java sources: {e}",
            javac.display()
        )),
        Ok(out) if !out.status.success() => error(format!(
            "javac could not compile the Java sources (a Java source that refers \
             to this compilation's Scala sources is not supported):\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )),
        Ok(_) if scala.is_empty() => CompileResult {
            diags: Vec::new(),
            sources: Vec::new(),
            emitted: Vec::new(),
            mains: Vec::new(),
        },
        Ok(_) => {
            let mut with_java = opts.clone();
            with_java.class_path.insert(0, dir.clone());
            compile_paths_unreported(scala, &with_java)
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn compile_paths_unreported(files: &[PathBuf], opts: &CompileOptions) -> CompileResult {
    let mut phase_start = std::env::var_os("SCALA_RS_PHASE_TIMING").map(|_| Instant::now());
    macro_rules! phase {
        ($name:literal) => {
            if let Some(start) = phase_start {
                eprintln!(
                    "[phase timing] {} {:.3} s",
                    $name,
                    start.elapsed().as_secs_f64()
                );
                phase_start = Some(Instant::now());
            }
        };
    }
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
                        library_abi: opts.scala_library.is_some(),
                        source3: opts.xsource3,
                        no_specialization: opts.no_specialization,
                        kind_projector: opts.kind_projector,
                        unicode_escapes_raw: opts
                            .source_features
                            .contains(SourceFeature::UnicodeEscapesRaw),
                    },
                );
                diags.extend(parsed.diags);
                units.push(Unit {
                    file_index,
                    tree: parsed.tree,
                    next_node_id: parsed.next_node_id,
                    pickles: std::rc::Rc::new(std::collections::HashMap::new()),
                    anonfun_names: Default::default(),
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
    phase!("parse");

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
    // Filled in just before erasure; `None` when the run stopped at a type
    // error, in which case nothing is emitted anyway.
    let mut generic_sigs: Option<std::rc::Rc<scala_rs_backend::GenericSignatures>> = None;
    {
        let classpath = match load_cp(&opts.class_path) {
            Ok(classpath) => classpath,
            Err(errors) => {
                diags.extend(errors.into_iter().map(|error| {
                    Diagnostic::error(
                        0,
                        Span::DUMMY,
                        format!("cannot load classpath ABI: {error}"),
                    )
                }));
                return failed_result(diags, sources);
            }
        };
        phase!("classpath");
        // One symbol table for the whole run: every unit is named before any
        // is typed, so files can reference each other.
        let node_id_bound = units.iter().map(|u| u.next_node_id).max();
        let mut refs: Vec<(&mut Tree, usize)> = units
            .iter_mut()
            .map(|u| {
                let fi = u.file_index;
                (&mut u.tree, fi)
            })
            .collect();
        let src_text: Vec<String> = sources.iter().map(|s| s.src.clone()).collect();
        let (mut st, tdiags) = typecheck_units_src(
            &mut refs,
            &TypecheckOptions {
                node_id_bound,
                // `-Werror` is applied by the reporting layer after the run
                // (`finish_diagnostics`), as nsc does: the typer's own
                // promotion of each warning to an error is for API callers.
                fatal_warnings: false,
                library_abi: opts.scala_library.is_some(),
                classpath,
                binary_path: {
                    let mut p = opts.class_path.clone();
                    if let Some(j) = &opts.scala_library {
                        p.push(j.clone());
                    }
                    p
                },
                macro_runtime: opts
                    .scala_library
                    .as_deref()
                    .map(find_macro_runtime)
                    .unwrap_or_default(),
                language_features: opts.language_features.clone(),
                source_features: opts.source_features,
                scala3: opts.xsource3,
                compiler_settings: compiler_settings(opts),
                source_paths: sources
                    .iter()
                    .map(|s| s.path.to_string_lossy().into_owned())
                    .collect(),
            },
            // The typer reads the text under a span for the forms the parser
            // folds together; `reify { … }` is the one whose body is *file*
            // text rather than a string the quasiquote machinery rebuilt.
            &src_text,
        );
        phase!("typecheck");
        diags.extend(tdiags);
        for u in units.iter() {
            mains.extend(find_mains(&st, &u.tree));
        }
        // A local `object` reading the enclosing instance or a captured local
        // is not compiled yet; say so rather than emitting a singleton that
        // crashes at run time.
        let local_classes = defines_local_classes(&st);
        if !has_errors(&diags) && local_classes {
            for u in units.iter() {
                diags.extend(check_local_objects(u.file_index, &u.tree, &st));
            }
        }
        if !has_errors(&diags) {
            for u in units.iter_mut() {
                u.anonfun_names =
                    std::rc::Rc::new(scala_rs_typer::anonfun_class_names(&u.tree, &st));
                // nsc's `NamesDefaults`: a call that omitted defaults binds its
                // qualifier to a local first, so the receiver is evaluated
                // once rather than once per `name$default$n` getter.
                hoist_default_receivers(&mut u.tree, &mut st);
                // …and, after it, nsc's other half of `NamesDefaults`: a call
                // whose named arguments were written out of parameter order
                // binds them to locals in the written order, so the reordering
                // the typer did to resolve the names is not observable.
                restore_named_arg_order(u.file_index, &mut u.tree, &mut st);
                // nsc evaluates the left operand of `a :: b` before `b`.
                restore_rassoc_order(&mut u.tree, &mut st);
                uncurry(&mut u.tree, &mut st);
                // A method-local `lazy val` becomes a cell plus a nested
                // accessor def; lambda-lift then hoists the accessor and
                // threads whatever the initialiser captured.
                lazy_locals(&mut u.tree, &mut st);
                lambda_lift(&mut u.tree, &mut st);
                mark_anon_captures(&u.tree, &mut st);
                // nsc `superaccessors`, which likewise runs before the
                // pickler: a `private` member reached from an anonymous /
                // local class, a lambda body or the companion is renamed
                // and published, because `ACC_PRIVATE` is per class file.
                expand_private_names(&mut u.tree, &mut st);
            }
            // A local `case class` whose companion would have to capture an
            // enclosing-method local is the same unimplemented `LazyRef`
            // shape as a capturing local `object` above, just reached
            // through `P(args)` and the synthetic companion instead of a
            // written body; `Symbol::captures` is only filled in by
            // `mark_anon_captures` just above, so this check has to run
            // here rather than alongside `check_local_objects`.
            if local_classes {
                for u in units.iter() {
                    diags.extend(check_local_case_class_captures(u.file_index, &u.tree, &st));
                }
            }
            // nsc's `extmethods` runs before `pickler`, so a value class's
            // `$extension` methods are part of the signature every later
            // compilation reads. Declare them on the companion module (and
            // synthesize that module when the source wrote none) before the
            // pickles are taken, or scalac reading our output asserts with
            // `no extension method found`.
            for u in units.iter() {
                add_value_class_companions(&u.tree, &mut st);
            }
            let mut source_classes = Vec::new();
            for u in units.iter() {
                collect_source_classes(&u.tree, &mut source_classes);
            }
            let mut output_owners: std::collections::HashSet<_> =
                source_classes.iter().copied().collect();
            for id in source_classes {
                output_owners.insert(st.module_class_of(id));
                if let Some(companion) = st.companion_module(id) {
                    output_owners.insert(companion);
                    output_owners.insert(st.module_class_of(companion));
                }
            }
            // Freeze source override identities while method types still
            // carry receiver and method type parameters. Backend mixin
            // dispatch runs after erasure and reads this table instead of
            // inferring a family from erased descriptors.
            scala_rs_typer::record_method_override_families_for(&mut st, &output_owners);
            // nsc `superaccessors`, likewise before `pickler`: which trait
            // members write `super.m` decides which `T$$super$m` members the
            // signature declares, and a reader implements exactly those.
            for u in units.iter() {
                scala_rs_backend::gen::mark_super_accessors(&u.tree, &mut st);
            }
            // Lazy classpath loading can allocate after source_start; those
            // classes are not emitted and do not need a new pickle.
            let emitted_symbols: Vec<_> = st
                .symbols
                .iter()
                .filter(|s| output_owners.contains(&s.id) || output_owners.contains(&s.owner))
                .map(|s| s.id)
                .collect();
            let pickles = std::rc::Rc::new(scala_rs_backend::pickle::pickle_selected(
                &st,
                emitted_symbols.iter().copied(),
            ));
            // nsc's specialize phase runs after pickler.  Keep the generic
            // declaration (and its @specialized type-parameter metadata) in
            // the source pickle, then add only the method-owned JVM entries
            // and rewrite concrete primitive call sites.  Collection and
            // rewriting are separate so calls can target a definition in a
            // later source unit.
            for u in units.iter_mut() {
                scala_rs_typer::specialize_method_defs(&mut u.tree, &mut st);
            }
            for u in units.iter_mut() {
                scala_rs_typer::rewrite_specialized_calls(&mut u.tree, &st);
            }
            // nsc's `mixin`, which runs *after* `pickler`: a trait's
            // unqualified-`private` `val` is renamed to `p$T$$v` in the class
            // files, but the pickle keeps the source name and the `private`
            // flag, because a reader (scalac included) derives the expansion
            // itself. Hence the position: between the pickle and codegen.
            for u in units.iter_mut() {
                expand_trait_private_vals(&mut u.tree, &mut st);
            }
            // Value classes are boxed across unit boundaries, so every unit's
            // declarations have to be known before the first one is erased.
            for u in units.iter() {
                note_source_value_classes(&u.tree, &mut st);
            }
            // JVMS 4.7.9 `Signature`: the generic shape of every member, read
            // while the types still carry it. `erase` below rewrites symbol
            // types in place, so this is the last moment the information
            // exists -- nsc reads it at the same point, `enteringErasure`.
            generic_sigs = Some(std::rc::Rc::new(
                scala_rs_backend::record_generic_signatures_for(&st, emitted_symbols),
            ));
            let preps: Vec<_> = units
                .iter_mut()
                .map(|u| scala_rs_typer::prepare_erasure(&mut u.tree, &mut st))
                .collect();
            let mut boxed = scala_rs_typer::BoxedParams::default();
            for (u, prep) in units.iter_mut().zip(preps) {
                u.pickles = std::rc::Rc::clone(&pickles);
                boxed.extend(scala_rs_typer::erase_prepared(&mut u.tree, &mut st, prep));
            }
            scala_rs_typer::rebox_value_class_lambda_params(&mut st, &boxed);
            scala_rs_typer::erase_default_getters(&mut st);
        }
        shared_st = Some(st);
    }
    phase!("lowering");

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
    let st = shared_st.as_ref().expect("the run is typed");
    // A class can mix in a trait defined in another file, so the concrete
    // trait members of the whole run have to be known before emitting any.
    let mut trait_members = scala_rs_backend::gen::TraitImpls::default();
    for u in &units {
        scala_rs_backend::gen::collect_trait_members(&u.tree, st, &mut trait_members);
    }
    // One map for the whole run, handed to each unit by reference count: it
    // holds every trait's concrete members and copying it per unit was 9% of
    // the compile.
    let trait_members = std::rc::Rc::new(trait_members);
    // Same story: a pure function of the (now frozen) symbol table, and it was
    // rebuilt from scratch for each of the run's units.
    let jvm_index = std::rc::Rc::new(scala_rs_backend::gen::build_jvm_index(st));
    // And again: the mutable locals a nested class captures are a property of
    // the run, and looking for them read every symbol once per unit.
    let captured_vars = std::rc::Rc::new(scala_rs_backend::gen::collect_captured_vars(st));
    // Third of the same kind: the case-class companion lookup in `emit_module`
    // was a linear search of the symbol table per module.
    let class_by_name = std::rc::Rc::new(scala_rs_backend::gen::build_class_name_index(st));
    // The class files behind `-cp` / `--scala-library`, read lazily and shared
    // by every unit: a class needs bridges for the erased overloads its
    // *binary* parents declare, and only the class files know what those are.
    // Without the jar there is nothing to read, so the private-runtime ABI
    // skips the pass.
    let binary_parents = opts.scala_library.as_ref().map(|j| {
        let mut p = opts.class_path.clone();
        p.push(j.clone());
        std::rc::Rc::new(scala_rs_backend::BinaryParents::new(p))
    });
    phase!("backend setup");

    // Each unit's classes go to the writer as soon as they exist, so the file
    // system works while later units are generated. A backend fallback is a
    // compile error, though, and publishing the runtime or an earlier unit
    // before a later unit fails would leave a partial class tree on disk: the
    // writer takes back what it wrote when any unit fails the backend gate.
    let mut writer = ClassWriter::start(&opts.out_dir);
    if !library_abi {
        writer.push(emit_runtime());
    }
    let mut emit_failed = false;
    for u in &units {
        let src_name = source_file_name(&sources[u.file_index]);
        let result = emit_opts(
            &u.tree,
            st,
            src_name,
            EmitOpts {
                library_abi,
                pickles: std::rc::Rc::clone(&u.pickles),
                trait_members: Some(std::rc::Rc::clone(&trait_members)),
                jvm_index: Some(std::rc::Rc::clone(&jvm_index)),
                captured_vars: Some(std::rc::Rc::clone(&captured_vars)),
                class_by_name: Some(std::rc::Rc::clone(&class_by_name)),
                binary_parents: binary_parents.clone(),
                generic_sigs: generic_sigs.clone(),
                anonfun_names: Some(std::rc::Rc::clone(&u.anonfun_names)),
            },
        );
        let mut classes = match result {
            Ok(classes) => classes,
            Err(errors) => {
                for e in errors {
                    diags.push(Diagnostic::error(u.file_index, e.span, e.message));
                }
                emit_failed = true;
                continue;
            }
        };
        // A class that exceeds a class file format limit is reported and
        // not written: the file would either be rejected by the loader or,
        // for the offsets that are only `u16`, quietly wrong. nsc reports
        // it the same way and writes nothing for the class either.
        classes.retain(|c| {
            if c.format_errors.is_empty() {
                return true;
            }
            let name = c.internal_name.replace('/', ".");
            for m in &c.format_errors {
                diags.push(
                    Diagnostic::error(
                        u.file_index,
                        Span::DUMMY,
                        format!("Error while emitting {name}"),
                    )
                    .note(m.clone()),
                );
            }
            emit_failed = true;
            false
        });
        if !emit_failed {
            writer.push(classes);
        }
    }
    phase!("emit");
    if emit_failed {
        writer.discard();
        return CompileResult {
            diags,
            sources,
            emitted: Vec::new(),
            mains,
        };
    }
    let (mut emitted, mut write_result) = writer.finish();
    // Fields are private until another class of the run is seen to read one
    // (`scala_rs_backend::field_access`); the few classes that changes are
    // written again.
    let reopened = scala_rs_backend::field_access::widen_cross_class_fields(&mut emitted);
    if write_result.is_ok() {
        let mut made = std::collections::HashSet::new();
        for i in reopened {
            if let Err(e) = write_class(&opts.out_dir, &mut made, &emitted[i]) {
                write_result = Err(e);
                break;
            }
        }
    }
    phase!("write");
    let _ = phase_start;
    if let Err(e) = write_result {
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

/// How many threads write class files.
///
/// Measured on slick (2127 files, 19 directories, APFS, `write_emitted` timed
/// on its own): 1 thread 110 ms, 2 threads 85 ms, **4 threads 55 ms**, 8
/// threads 95 ms, 12 threads 180 ms, 16 threads 190 ms, 32 threads 200 ms.
///
/// Creating a file takes an exclusive lock on its directory, and slick's
/// classes land in 19 of them (716 in one). Threads past a handful therefore
/// queue on each other, and what is left is context switching. "It is I/O
/// bound, so add threads" is the wrong model here.
const WRITE_JOBS: usize = 4;

/// [`WRITE_JOBS`], never more than the machine has cores.
fn write_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().min(WRITE_JOBS))
        .unwrap_or(1)
        .max(1)
}

/// Runs below this many classes write on the calling thread: starting the pool
/// costs more than the writes save.
const WRITE_POOL_MIN: usize = 64;

/// Write one class to `out_dir/{internal_name}.class`.
///
/// `made` remembers the package directories this writer has already created.
/// `create_dir_all` walks and stats the whole chain on every call and nothing
/// removes a directory while a compile runs, so once per directory is enough.
fn write_class(
    out_dir: &Path,
    made: &mut std::collections::HashSet<PathBuf>,
    class: &EmittedClass,
) -> std::io::Result<()> {
    let dest = class_path(out_dir, &class.internal_name);
    if let Some(parent) = dest.parent() {
        if !made.contains(parent) {
            std::fs::create_dir_all(parent)?;
            made.insert(parent.to_path_buf());
        }
    }
    std::fs::write(&dest, &class.bytes)
}

type Chunk = (usize, Vec<EmittedClass>);

/// The writer pool, once it has been started.
struct WritePool {
    jobs: std::sync::mpsc::Sender<Chunk>,
    done: std::sync::mpsc::Receiver<Chunk>,
    threads: Vec<std::thread::JoinHandle<()>>,
    failed: std::sync::Arc<std::sync::Mutex<Option<std::io::Error>>>,
}

/// Writes class files in the background while the compile keeps generating
/// them.
///
/// The caller pushes one unit's classes at a time and gets them all back, in
/// push order, from [`ClassWriter::finish`]. Each chunk is moved to a writer
/// thread and moved back when it has been written, so nothing is copied and
/// nothing is shared: the classes a caller sees are the ones it emitted.
///
/// Writing every class after the last unit made the file system latency
/// (almost all of it in `open`) wall time nobody was doing anything else
/// during. Overlapping it with code generation hides it instead: 0.55 s of
/// lambdas_big's 12202 classes.
///
/// A compile that fails in a later unit leaves no class file of the run
/// behind: [`ClassWriter::discard`] deletes the ones already written. Staging
/// them in a directory of their own and renaming them into place cost as much
/// as the writes when a run's classes share one directory.
struct ClassWriter {
    out_dir: PathBuf,
    /// Chunks held back until the run is known to be worth a thread pool.
    pending: Vec<Vec<EmittedClass>>,
    pending_classes: usize,
    pool: Option<WritePool>,
    next_seq: usize,
}

impl ClassWriter {
    fn start(out_dir: &Path) -> ClassWriter {
        ClassWriter {
            out_dir: out_dir.to_path_buf(),
            pending: Vec::new(),
            pending_classes: 0,
            pool: None,
            next_seq: 0,
        }
    }

    /// Hand one unit's classes over to be written.
    fn push(&mut self, classes: Vec<EmittedClass>) {
        if classes.is_empty() {
            return;
        }
        if let Some(pool) = &self.pool {
            let seq = self.next_seq;
            self.next_seq += 1;
            // The only way this fails is every writer having died, which cannot
            // happen while `pool.jobs` is alive.
            let _ = pool.jobs.send((seq, classes));
            return;
        }
        self.pending_classes += classes.len();
        self.pending.push(classes);
        if self.pending_classes >= WRITE_POOL_MIN {
            self.start_pool();
        }
    }

    fn start_pool(&mut self) {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<Chunk>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<Chunk>();
        let job_rx = std::sync::Arc::new(std::sync::Mutex::new(job_rx));
        let failed: std::sync::Arc<std::sync::Mutex<Option<std::io::Error>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let jobs = write_jobs();
        let mut threads = Vec::with_capacity(jobs);
        for _ in 0..jobs {
            let job_rx = std::sync::Arc::clone(&job_rx);
            let failed = std::sync::Arc::clone(&failed);
            let done_tx = done_tx.clone();
            let out_dir = self.out_dir.clone();
            threads.push(std::thread::spawn(move || {
                // Per thread, so no lock is taken on the common path. Four
                // threads over 19 directories is at most 76 `create_dir_all`
                // calls for a whole run.
                let mut made = std::collections::HashSet::new();
                loop {
                    // Held only across `recv`: a worker that has a chunk drops
                    // the lock before writing it.
                    let job = {
                        let rx = job_rx.lock().unwrap_or_else(|e| e.into_inner());
                        rx.recv()
                    };
                    let Ok((seq, classes)) = job else { return };
                    let broken = failed.lock().unwrap_or_else(|e| e.into_inner()).is_some();
                    if !broken {
                        for class in &classes {
                            if let Err(e) = write_class(&out_dir, &mut made, class) {
                                let mut slot = failed.lock().unwrap_or_else(|e| e.into_inner());
                                if slot.is_none() {
                                    *slot = Some(e);
                                }
                                break;
                            }
                        }
                    }
                    // Give the classes back even when a write failed: the
                    // caller still reports what was generated.
                    let _ = done_tx.send((seq, classes));
                }
            }));
        }
        drop(done_tx);
        self.pool = Some(WritePool {
            jobs: job_tx,
            done: done_rx,
            threads,
            failed,
        });
        self.pending_classes = 0;
        for classes in std::mem::take(&mut self.pending) {
            self.push(classes);
        }
    }

    /// Delete every class file written so far, and the directories that
    /// leaves empty.
    fn discard(self) {
        // Chunks still pending never started the pool and were not written.
        let Some(pool) = self.pool else { return };
        let out_dir = self.out_dir;
        let (written, _) = pool.join();
        let mut dirs = std::collections::BTreeSet::new();
        for class in &written {
            let path = class_path(&out_dir, &class.internal_name);
            if std::fs::remove_file(&path).is_ok() {
                let mut dir = path.parent();
                while let Some(d) = dir.filter(|d| *d != out_dir) {
                    dirs.insert(d.to_path_buf());
                    dir = d.parent();
                }
            }
        }
        // Deepest first; one that still holds files of an earlier build
        // stays.
        for d in dirs.iter().rev() {
            let _ = std::fs::remove_dir(d);
        }
    }

    /// Wait for every class to be on disk and return them all in push order.
    fn finish(self) -> (Vec<EmittedClass>, std::io::Result<()>) {
        let ClassWriter {
            out_dir,
            pending,
            pool,
            ..
        } = self;
        let Some(pool) = pool else {
            // Too few classes to have started the pool.
            let mut made = std::collections::HashSet::new();
            let mut out = Vec::new();
            let mut result = Ok(());
            for classes in pending {
                for class in &classes {
                    if result.is_ok() {
                        result = write_class(&out_dir, &mut made, class);
                    }
                }
                out.extend(classes);
            }
            return (out, result);
        };
        pool.join()
    }
}

impl WritePool {
    /// Wait for every chunk to be written and return them in push order.
    fn join(self) -> (Vec<EmittedClass>, std::io::Result<()>) {
        // Closing the queue is what tells the writers to stop.
        drop(self.jobs);
        for t in self.threads {
            let _ = t.join();
        }
        // Every sender is gone now, so this drains and ends.
        let mut chunks: Vec<Chunk> = self.done.into_iter().collect();
        chunks.sort_by_key(|(seq, _)| *seq);
        let mut out = Vec::with_capacity(chunks.iter().map(|(_, c)| c.len()).sum());
        for (_, classes) in chunks {
            out.extend(classes);
        }
        let failed = self.failed.lock().unwrap_or_else(|e| e.into_inner()).take();
        match failed {
            Some(e) => (out, Err(e)),
            None => (out, Ok(())),
        }
    }
}

/// Write each class to `out_dir/{internal_name}.class`, creating package
/// subdirectories as needed (`foo/Bar` → `out_dir/foo/Bar.class`).
///
/// [`compile_paths`] does not go through this: it streams each unit's classes
/// to a [`ClassWriter`] as they are generated. This is for callers that already
/// hold every class.
pub fn write_emitted(emitted: &[EmittedClass], out_dir: &Path) -> std::io::Result<()> {
    if emitted.is_empty() {
        return Ok(());
    }
    if emitted.len() < WRITE_POOL_MIN {
        let mut made = std::collections::HashSet::new();
        for class in emitted {
            write_class(out_dir, &mut made, class)?;
        }
        return Ok(());
    }
    // One `open`/`write`/`close` per class, each blocking on the file system.
    // Deterministic output: the file set and each file's bytes do not depend on
    // the interleaving.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failed: std::sync::Mutex<Option<std::io::Error>> = std::sync::Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..write_jobs() {
            s.spawn(|| {
                let mut made = std::collections::HashSet::new();
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= emitted.len() {
                        return;
                    }
                    if let Err(e) = write_class(out_dir, &mut made, &emitted[i]) {
                        let mut slot = failed.lock().unwrap_or_else(|e| e.into_inner());
                        if slot.is_none() {
                            *slot = Some(e);
                        }
                        return;
                    }
                }
            });
        }
    });
    match failed.into_inner().unwrap_or_else(|e| e.into_inner()) {
        Some(e) => Err(e),
        None => Ok(()),
    }
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

fn load_cp(
    paths: &[PathBuf],
) -> Result<Vec<scala_rs_typer::ClasspathClass>, Vec<scala_rs_backend::ClasspathLoadError>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    // BinaryIndex loads directory and archive entries on demand. Eagerly
    // decoding every directory class duplicates that work for each consumer,
    // including dependencies whose declarations the source never uses.
    // Explicit class files still need this loader and its strict diagnostics.
    let explicit: Vec<_> = paths
        .iter()
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("class") && !p.is_dir())
        .collect();
    load_classpath_checked(&explicit).map(scala_rs_typer::adapt_classpath)
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

/// The scala-reflect and scala-compiler jars of `library`'s version, which
/// nsc runs macro implementations with: beside the library jar, under
/// `$SCALA_HOME/lib` or the `/tmp/scala-<version>` distribution the tests use,
/// or in the Coursier cache. The ones not found are left out; a macro that
/// needs one then says which is missing.
pub fn find_macro_runtime(library: &Path) -> Vec<PathBuf> {
    let version = library
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("scala-library-"))
        .and_then(|n| n.strip_suffix(".jar"))
        .unwrap_or("2.13.16")
        .to_string();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = library.parent() {
        dirs.push(dir.to_path_buf());
    }
    if let Some(home) = std::env::var_os("SCALA_HOME") {
        dirs.push(PathBuf::from(home).join("lib"));
    }
    dirs.push(PathBuf::from(format!("/tmp/scala-{version}/lib")));
    let caches: Vec<PathBuf> = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| {
            vec![
                home.join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
                home.join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
            ]
        })
        .unwrap_or_default();
    ["scala-reflect", "scala-compiler"]
        .into_iter()
        .filter_map(|kind| {
            let versioned = format!("{kind}-{version}.jar");
            dirs.iter()
                .flat_map(|d| [d.join(&versioned), d.join(format!("{kind}.jar"))])
                .chain(caches.iter().map(|c| {
                    c.join("org/scala-lang")
                        .join(kind)
                        .join(&version)
                        .join(&versioned)
                }))
                .find(|p| p.is_file())
        })
        .collect()
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

    #[test]
    fn macro_runtime_is_found_beside_the_library_jar() {
        let dir = fresh_dir();
        let lib = dir.0.join("scala-library-2.13.99.jar");
        std::fs::write(&lib, b"").unwrap();
        // Nothing of version 2.13.99 anywhere else on the machine.
        assert!(find_macro_runtime(&lib).is_empty());
        let reflect = dir.0.join("scala-reflect-2.13.99.jar");
        let compiler = dir.0.join("scala-compiler-2.13.99.jar");
        std::fs::write(&reflect, b"").unwrap();
        std::fs::write(&compiler, b"").unwrap();
        assert_eq!(find_macro_runtime(&lib), vec![reflect, compiler]);
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
                format_errors: Vec::new(),
            },
            EmittedClass {
                internal_name: "foo/Bar".into(),
                bytes: vec![0xBA, 0xBE],
                format_errors: Vec::new(),
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
            deprecation: false,
            feature: false,
            nowarn: false,
            scala_library: None,
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
            source_features: SourceFeatures::default(),
            xasync: false,
            no_specialization: false,
            kind_projector: false,
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
            deprecation: false,
            feature: false,
            nowarn: false,
            scala_library: None,
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
            source_features: SourceFeatures::default(),
            xasync: false,
            no_specialization: false,
            kind_projector: false,
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
            deprecation: false,
            feature: false,
            nowarn: false,
            scala_library: None,
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
            source_features: SourceFeatures::default(),
            xasync: false,
            no_specialization: false,
            kind_projector: false,
        };
        let result = compile_paths(&[src], &opts);
        assert!(!result.ok());
        assert!(result.emitted.is_empty());
        assert!(result.diags.iter().any(|d| d.message.contains("not found")));
    }

    #[test]
    fn malformed_classpath_class_is_reported() {
        let tmp = fresh_dir();
        let src = tmp.0.join("Main.scala");
        let bad_class = tmp.0.join("Broken.class");
        std::fs::write(&src, "object Main {}\n").unwrap();
        std::fs::write(&bad_class, b"not a classfile").unwrap();
        let opts = CompileOptions {
            out_dir: tmp.0.join("out"),
            class_path: vec![bad_class],
            ..CompileOptions::default()
        };

        let result = compile_paths(&[src], &opts);

        assert!(!result.ok());
        assert!(result.emitted.is_empty());
        assert!(result.diags.iter().any(|diag| {
            diag.message.contains("cannot load classpath ABI")
                && diag.message.contains("not a classfile (bad magic)")
        }));
    }

    #[test]
    fn directory_classpath_is_left_to_lazy_binary_loading() {
        let tmp = fresh_dir();
        let src = tmp.0.join("Library.scala");
        std::fs::write(
            &src,
            "package lazycp; class Library { def value: Int = 42 }",
        )
        .unwrap();
        let producer = CompileOptions {
            out_dir: tmp.0.join("library"),
            ..CompileOptions::default()
        };
        let result = compile_paths(&[src], &producer);
        assert!(result.ok(), "{}", result.render_diags());
        assert!(load_cp(&[producer.out_dir.clone()]).unwrap().is_empty());
        assert_eq!(
            load_cp(&[producer.out_dir.join("lazycp/Library.class")])
                .unwrap()
                .len(),
            1
        );

        let src = tmp.0.join("Consumer.scala");
        std::fs::write(
            &src,
            "object Consumer { def value: Int = new lazycp.Library().value }",
        )
        .unwrap();
        let consumer = CompileOptions {
            out_dir: tmp.0.join("consumer"),
            class_path: vec![producer.out_dir],
            ..CompileOptions::default()
        };
        let result = compile_paths(&[src], &consumer);
        assert!(result.ok(), "{}", result.render_diags());
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
            deprecation: false,
            feature: false,
            nowarn: false,
            scala_library: Some(PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar")),
            class_path: Vec::new(),
            language_features: Vec::new(),
            xsource3: false,
            source_features: SourceFeatures::default(),
            xasync: false,
            no_specialization: false,
            kind_projector: false,
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
