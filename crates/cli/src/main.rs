//! scala-rs command-line compiler (Scala 2.13 subset, not Scala 3).

/// The compiler is allocation-bound: 42% of samples in a `sample` profile of a
/// 184-file slick build were in the system allocator. macOS's libmalloc pays a
/// lock on every small allocation; mimalloc's thread-local free lists do not.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use scala_rs_driver::{
    compile_paths, enable_resident_archive_cache, enable_resident_direct_macro, find_scala_library,
    find_scala_xml, run_main_with_cp, CompileOptions, CompileResult, SourceFeatures,
};
use scala_rs_span::{render_all, render_scalac};

fn main() -> ExitCode {
    // Deeply nested types and long method chains recurse; the default 8 MB
    // main-thread stack is not enough for a real project.
    std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(run)
        .expect("spawn compiler thread")
        .join()
        .unwrap_or(ExitCode::from(2))
}

fn run() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_help();
        return ExitCode::from(1);
    }

    if wants_help(&args) {
        print_help();
        return ExitCode::SUCCESS;
    }

    let cmd = args.remove(0);
    match cmd.as_str() {
        "__macro_proxy" => macro_proxy(&args),
        "__macro_endpoint" => macro_endpoint(&args),
        "__compile_batch" => compile_batch(&args),
        "compile" => cmd_compile(&args),
        "run" => cmd_run(&args),
        "help" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown command '{other}'");
            eprintln!();
            print_help();
            ExitCode::from(1)
        }
    }
}

fn macro_endpoint(args: &[String]) -> ExitCode {
    if let [dir, classpath] = args {
        match macro_daemon_endpoint(dir, classpath) {
            Ok(endpoint) => {
                println!("{}", endpoint.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(2)
            }
        }
    } else {
        ExitCode::from(2)
    }
}

fn macro_proxy(args: &[String]) -> ExitCode {
    use std::io::{BufRead, BufReader};
    use std::net::{TcpStream, ToSocketAddrs};

    let (endpoint_path, classpath) = if args.len() == 3 && args[0] == "--auto" {
        match macro_daemon_endpoint(&args[1], &args[2]) {
            Ok(endpoint) => (endpoint, args[2].as_str()),
            Err(error) => {
                if std::env::var_os("SCALA_RS_MACRO_DAEMON_REQUIRE")
                    .is_some_and(|value| value == "1")
                {
                    eprintln!("macro daemon required: {error}");
                    return ExitCode::from(2);
                }
                return macro_proxy_direct(&args[1], &args[2]);
            }
        }
    } else if args.len() == 2 {
        (PathBuf::from(&args[0]), args[1].as_str())
    } else {
        return ExitCode::from(2);
    };
    let endpoint = match std::fs::read_to_string(&endpoint_path) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("cannot read macro daemon endpoint: {error}");
            return ExitCode::from(2);
        }
    };
    let mut endpoint_fields = endpoint.split_whitespace();
    let (Some(port), Some(token)) = (endpoint_fields.next(), endpoint_fields.next()) else {
        eprintln!("malformed macro daemon endpoint");
        return ExitCode::from(2);
    };
    let address = format!("127.0.0.1:{port}");
    let mut addresses = match address.to_socket_addrs() {
        Ok(addresses) => addresses,
        Err(error) => {
            eprintln!("invalid macro daemon endpoint: {error}");
            return ExitCode::from(2);
        }
    };
    let Some(address) = addresses.next() else {
        return ExitCode::from(2);
    };
    let mut socket = match TcpStream::connect_timeout(&address, std::time::Duration::from_secs(5)) {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("cannot connect to macro daemon: {error}");
            return ExitCode::from(2);
        }
    };
    let _ = socket.set_nodelay(true);
    let mut start = String::from("(start ");
    quote_macro_wire(&mut start, token);
    for entry in classpath.split(if cfg!(windows) { ';' } else { ':' }) {
        start.push(' ');
        quote_macro_wire(&mut start, entry);
    }
    start.push_str(")\n");
    if socket.write_all(start.as_bytes()).is_err() {
        return ExitCode::from(2);
    }
    let reply_socket = match socket.try_clone() {
        Ok(reply_socket) => reply_socket,
        Err(_) => return ExitCode::from(2),
    };
    let mut replies = BufReader::new(reply_socket);
    let mut hello = String::new();
    if replies.read_line(&mut hello).is_err() || hello.is_empty() {
        return ExitCode::from(2);
    }
    if std::io::stdout().write_all(hello.as_bytes()).is_err() {
        return ExitCode::from(2);
    }
    if hello.trim_end() != "(ready)" {
        return ExitCode::from(2);
    }
    let _input = std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut socket);
        let _ = socket.shutdown(std::net::Shutdown::Write);
    });
    let result = std::io::copy(&mut replies, &mut std::io::stdout());
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}

fn macro_proxy_direct(engine_dir: &str, classpath: &str) -> ExitCode {
    use std::process::{Command, Stdio};

    let mut child = match Command::new(macro_java())
        .arg("-cp")
        .arg(format!(
            "{engine_dir}{}{classpath}",
            if cfg!(windows) { ';' } else { ':' }
        ))
        .arg("ScalaRsMacroEngine")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("cannot start macro engine: {error}");
            return ExitCode::from(2);
        }
    };
    let mut engine_input = child.stdin.take().expect("piped stdin");
    let mut engine_output = child.stdout.take().expect("piped stdout");
    let _input = std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut engine_input);
    });
    let result = std::io::copy(&mut engine_output, &mut std::io::stdout());
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}

fn macro_java() -> PathBuf {
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let candidate = PathBuf::from(home).join("bin/java");
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("java")
}

#[cfg(unix)]
fn macro_daemon_endpoint(engine_dir: &str, classpath: &str) -> Result<PathBuf, String> {
    use std::io::Read;
    use std::net::{SocketAddr, TcpStream};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    unsafe extern "C" {
        fn flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
        fn geteuid() -> u32;
    }
    const LOCK_EX: std::os::raw::c_int = 2;

    let separator = if cfg!(windows) { ';' } else { ':' };
    let runtime: Vec<&str> = classpath
        .split(separator)
        .filter(|entry| {
            let name = std::path::Path::new(entry)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            name.starts_with("scala-library")
                || name.starts_with("scala-reflect")
                || name.starts_with("scala-compiler")
        })
        .collect();
    if !runtime.iter().any(|entry| entry.contains("scala-reflect")) {
        return Err("scala-reflect is missing from the macro classpath".into());
    }
    let runtime_cp = runtime.join(":");
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in runtime_cp
        .as_bytes()
        .iter()
        .chain(
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .as_bytes(),
        )
        .chain(macro_java().to_string_lossy().as_bytes())
    {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    let private = PathBuf::from(engine_dir).join(format!("daemon-{hash:016x}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&private) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("cannot create daemon directory: {error}")),
    }
    let metadata = std::fs::symlink_metadata(&private).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err("macro daemon directory is not private".into());
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(private.join("lock"))
        .map_err(|e| e.to_string())?;
    // SAFETY: the live file descriptor remains open for the whole critical section.
    if unsafe { flock(lock.as_raw_fd(), LOCK_EX) } != 0 {
        return Err(format!(
            "cannot lock macro daemon: {}",
            std::io::Error::last_os_error()
        ));
    }
    let endpoint = private.join("endpoint");
    let live = |endpoint: &std::path::Path| -> bool {
        let Ok(info) = std::fs::read_to_string(endpoint) else {
            return false;
        };
        let Some((port, _)) = info.trim().split_once(' ') else {
            return false;
        };
        let Ok(address) = format!("127.0.0.1:{port}").parse::<SocketAddr>() else {
            return false;
        };
        TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok()
    };
    if live(&endpoint) {
        return Ok(endpoint);
    }
    let mut socket_hash = hash;
    for byte in engine_dir.as_bytes() {
        socket_hash ^= *byte as u64;
        socket_hash = socket_hash.wrapping_mul(0x100_0000_01b3);
    }
    let socket_dir = PathBuf::from("/tmp").join(format!("scala-rs-daemon-{socket_hash:016x}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&socket_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("cannot create macro socket directory: {error}")),
    }
    let socket_metadata = std::fs::symlink_metadata(&socket_dir).map_err(|e| e.to_string())?;
    // SAFETY: geteuid has no preconditions.
    if !socket_metadata.is_dir()
        || socket_metadata.uid() != unsafe { geteuid() }
        || socket_metadata.permissions().mode() & 0o077 != 0
    {
        return Err("macro socket directory is not private".into());
    }
    let socket_path = socket_dir.join("socket");
    if endpoint.exists() {
        std::fs::remove_file(&endpoint).map_err(|e| e.to_string())?;
    }
    let mut random = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|e| e.to_string())?;
    let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut daemon = Command::new(macro_java());
    daemon
        .arg("-Xmx2g")
        .arg("-cp")
        .arg(engine_dir)
        .arg("ScalaRsMacroEngine")
        .arg("--daemon")
        .arg(&endpoint)
        .arg(&token)
        .arg(&socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    daemon.process_group(0);
    let mut child = daemon
        .spawn()
        .map_err(|e| format!("cannot start macro daemon: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if live(&endpoint) {
            return Ok(endpoint);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("macro daemon exited at startup: {status}"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("macro daemon startup timed out".into())
}

#[cfg(not(unix))]
fn macro_daemon_endpoint(_engine_dir: &str, _classpath: &str) -> Result<PathBuf, String> {
    Err("macro daemon reuse is unavailable on this platform".into())
}

fn quote_macro_wire(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('"');
}

fn wants_help(args: &[String]) -> bool {
    for a in args {
        if a == "--" {
            return false;
        }
        if a == "--help" || a == "-h" {
            return true;
        }
    }
    false
}

fn print_help() {
    println!(
        "\
scala-rs — a Scala 2.13 subset compiler (not Scala 3)

USAGE:
    scala-rs compile <files...> [-d <dir>] [-cp <path>] [--scala-library <jar>] [--no-scala-library] [--parse] [--typer] [-Xfatal-warnings] [-deprecation] [-feature] [-nowarn] [--diagnostics=scalac] [-language:<feat>] [-Xsource:3] [-Xsource-features:<features>] [-Xasync] [-no-specialization] [-Ykind-projector]
    scala-rs run <file> [--scala-library <jar>] [--no-scala-library] [--] [java-args...]
    scala-rs --help

This is an experimental reimplementation of a Scala 2.13 (nsc) subset.
Scala 3 syntax and TASTy are not supported.

COMMANDS:
    compile    Compile Scala sources to JVM class files
    run        Compile a file to a temp directory and run its main method

OPTIONS:
    -d <dir>   Output directory for class files (default: .)
    -cp <path> Classpath of previously compiled class files (`:`-separated)
    --class-path <path>
               Same as -cp
    --scala-library [<jar>]
               Link against scala-library 2.13 (do not emit private Option/List).
               Path optional: searches SCALA_LIBRARY_JAR, /tmp/scala-rs-lib, cwd.
               `compile` and `run` auto-use a found 2.13 jar by default.
    --no-scala-library
               Force the private runtime even if a jar is auto-found.
    --parse             Parse only and dump the AST (do not typecheck or emit)
    --typer             Dump the typed tree after namer/typer
    -Xfatal-warnings, -Werror
                        Fail the compilation if there are any warnings (they
                        are still reported as warnings, as nsc does)
    -deprecation        Report each deprecation instead of a summary line
    -feature            Report each feature warning instead of a summary line
    -nowarn             Report no warnings
    -unchecked          Accepted; unchecked warnings are on by default
    --diagnostics=scalac
                        Print diagnostics as scalac's console reporter does
                        (`file:line: warning: msg`, source line, caret, and the
                        `N warnings` / `N errors` counts) and nothing else
    -language:<feat>    Enable a language feature (`postfixOps`, `implicitConversions`, `dynamics`)
    -Xsource:<version>  Source level: `2.13` (default), `3`, or `3-cross`.
                        `3`/`3-cross` accept the Scala 3 spellings this subset
                        implements (`A & B` compound types).
    -Xsource-features:<features>
                        Enable Scala 3 behaviours under -Xsource:3 (ignored,
                        with a warning, without it). `3-cross` is `3` plus
                        every feature. `-Xsource-features:help` lists them;
                        `case-apply-copy-access` is the one implemented here.
    -no-specialization  Ignore `@specialized` / `@unspecialized` (nsc's flag of
                        the same name). Without it the implemented method-owned
                        Int/Long entries are emitted; class/trait entries remain
                        outside this phase.
    -Ykind-projector    Accept the kind-projector plugin's type-lambda syntax:
                        the `*` placeholder (`Either[E, *]`, `(A, *)`, `A => *`)
                        and `λ[α => F[α]]` / `Lambda[(A, B) => F[B, A]]`. NOT an
                        nsc flag -- kind-projector is a compiler plugin, and nsc
                        without it rejects all of this, so the default is off.
                        The name is Scala 3's flag for the same syntax.
    -Xasync             Enable nonblocking scala.async.Async async/await.
                        Requires the scala-async jar and --scala-library.
    --help              Show this help

EXAMPLES:
    scala-rs compile Main.scala -d out
    scala-rs compile Main.scala --scala-library scala-library-2.13.16.jar -d out
    scala-rs compile Main.scala --no-scala-library -d out
    scala-rs compile Main.scala --parse
    scala-rs run Main.scala
    scala-rs run Main.scala -- arg1 arg2
"
    );
}

fn print_diags(result: &CompileResult) {
    if result.diags.is_empty() {
        return;
    }
    eprint!("{}", render_all(&result.diags, &result.sources));
}

fn cmd_compile(args: &[String]) -> ExitCode {
    compile_args(args, false)
}

/// Process a stream of NUL-delimited argument lists in one compiler process.
/// Each list ends with an extra NUL; one status byte is written after its
/// diagnostics and class files are complete. The caller owns dependency order.
fn compile_batch(options: &[String]) -> ExitCode {
    const MAX_ARGUMENT_BYTES: usize = 16 * 1024 * 1024;
    const MAX_ARGUMENTS: usize = 100_000;
    let cache_enabled = match options {
        [] => true,
        [option] if option == "--no-archive-cache" => false,
        _ => {
            eprintln!("error: unknown batch compilation option");
            return ExitCode::from(2);
        }
    };
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    if cache_enabled {
        enable_resident_archive_cache();
    }
    enable_resident_direct_macro();
    let mut args = Vec::new();
    loop {
        let mut bytes = Vec::new();
        match input
            .by_ref()
            .take(MAX_ARGUMENT_BYTES as u64 + 1)
            .read_until(0, &mut bytes)
        {
            Ok(0) if args.is_empty() => return ExitCode::SUCCESS,
            Ok(0) => {
                eprintln!("error: incomplete batch compilation request");
                return ExitCode::from(2);
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("error: cannot read batch compilation request: {error}");
                return ExitCode::from(2);
            }
        }
        if bytes.len() > MAX_ARGUMENT_BYTES || bytes.last() != Some(&0) {
            eprintln!("error: oversized or unterminated batch compilation argument");
            return ExitCode::from(2);
        }
        bytes.pop();
        if bytes.is_empty() {
            if args.is_empty() {
                eprintln!("error: empty batch compilation request");
                return ExitCode::from(2);
            }
            let status = compile_args(&args, true);
            args.clear();
            if output
                .write_all(&[u8::from(status != ExitCode::SUCCESS)])
                .is_err()
                || output.flush().is_err()
            {
                return ExitCode::from(2);
            }
            continue;
        }
        if args.len() >= MAX_ARGUMENTS {
            eprintln!("error: too many batch compilation arguments");
            return ExitCode::from(2);
        }
        match String::from_utf8(bytes) {
            Ok(arg) => args.push(arg),
            Err(_) => {
                eprintln!("error: batch compilation argument is not UTF-8");
                return ExitCode::from(2);
            }
        }
    }
}

fn compile_args(args: &[String], quiet: bool) -> ExitCode {
    let parsed = match parse_compile_args(args) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(1);
        }
    };
    if quiet && (parsed.features_help || parsed.opts.parse_only || parsed.opts.typer_dump) {
        eprintln!("error: batch compilation only supports class-file output");
        return ExitCode::from(1);
    }
    if parsed.features_help {
        print!("{}", SourceFeatures::help_text());
        return ExitCode::SUCCESS;
    }
    for w in &parsed.warnings {
        eprintln!("warning: {w}");
    }
    if parsed.files.is_empty() {
        eprintln!("error: no input files");
        return ExitCode::from(1);
    }

    let result = compile_paths(&parsed.files, &parsed.opts);
    if parsed.scalac_diagnostics {
        eprint!("{}", render_scalac(&result.diags, &result.sources));
    } else {
        print_diags(&result);
    }
    if !result.ok() {
        return ExitCode::from(1);
    }

    if !quiet && !parsed.opts.parse_only && !parsed.scalac_diagnostics {
        let n = result.emitted.len();
        println!(
            "wrote {n} class file{} to {}",
            if n == 1 { "" } else { "s" },
            parsed.opts.out_dir.display()
        );
    }
    ExitCode::SUCCESS
}

struct CompileArgs {
    files: Vec<PathBuf>,
    opts: CompileOptions,
    /// Settings-level warnings (nsc reports these before it reads any source).
    warnings: Vec<String>,
    /// `-Xsource-features:help` was asked for; print the list and stop.
    features_help: bool,
    /// `--diagnostics=scalac`: print diagnostics exactly as scalac's
    /// `ConsoleReporter` does, and nothing else.
    scalac_diagnostics: bool,
}

fn parse_compile_args(args: &[String]) -> Result<CompileArgs, String> {
    let mut out_dir = PathBuf::from(".");
    let mut parse_only = false;
    let mut typer_dump = false;
    let mut fatal_warnings = false;
    let mut deprecation = false;
    let mut feature = false;
    let mut nowarn = false;
    let mut scalac_diagnostics = false;
    let mut scala_library = None;
    let mut no_scala_library = false;
    let mut class_path = Vec::new();
    let mut language_features = Vec::new();
    let mut xsource3 = false;
    let mut xsource_cross = false;
    let mut named_features = SourceFeatures::default();
    let mut named_features_given = false;
    let mut unimplemented_features: Vec<&'static str> = Vec::new();
    let mut features_help = false;
    let mut xasync = false;
    let mut no_specialization = false;
    let mut kind_projector = false;
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            files.extend(args[i + 1..].iter().map(PathBuf::from));
            break;
        } else if a == "-deprecation" {
            // Before the `-d<dir>` spelling below, which would read this as
            // the directory `eprecation`.
            deprecation = true;
        } else if a == "-d" {
            i += 1;
            let dir = args
                .get(i)
                .ok_or_else(|| "option -d requires a directory argument".to_string())?;
            out_dir = PathBuf::from(dir);
        } else if let Some(dir) = a.strip_prefix("-d") {
            if dir.is_empty() {
                return Err("option -d requires a directory argument".into());
            }
            out_dir = PathBuf::from(dir);
        } else if a == "--parse" {
            parse_only = true;
        } else if a == "--typer" {
            typer_dump = true;
        } else if a == "-Xfatal-warnings" || a == "-Werror" {
            fatal_warnings = true;
        } else if a == "-feature" {
            feature = true;
        } else if a == "-nowarn" {
            nowarn = true;
        } else if a == "-unchecked" {
            // nsc 2.13 reports unchecked warnings by default; the flag only
            // restates that.
        } else if a == "--diagnostics=scalac" {
            scalac_diagnostics = true;
        } else if a == "--diagnostics=rust" {
            scalac_diagnostics = false;
        } else if a == "--no-scala-library" {
            no_scala_library = true;
        } else if a == "--scala-library" || a.starts_with("--scala-library=") {
            scala_library = Some(take_scala_library_flag(args, &mut i)?);
        } else if a == "-cp" || a == "--class-path" || a == "-classpath" {
            i += 1;
            let cp = args
                .get(i)
                .ok_or_else(|| "option -cp requires a classpath argument".to_string())?;
            class_path.extend(split_classpath(cp));
        } else if let Some(rest) = a.strip_prefix("-language:") {
            if rest.is_empty() {
                return Err("option -language: requires a feature name".into());
            }
            for feat in rest.split(',') {
                let f = feat.trim();
                if !f.is_empty() {
                    language_features.push(f.to_string());
                }
            }
        } else if let Some(rest) = a.strip_prefix("-Xsource-features:") {
            let parsed = SourceFeatures::parse(rest)?;
            named_features = parsed.features;
            named_features_given = true;
            features_help |= parsed.help;
            unimplemented_features.extend(parsed.unimplemented);
        } else if a == "-Xsource-features" {
            i += 1;
            let spec = args.get(i).ok_or_else(|| {
                "option -Xsource-features requires a feature argument".to_string()
            })?;
            let parsed = SourceFeatures::parse(spec)?;
            named_features = parsed.features;
            named_features_given = true;
            features_help |= parsed.help;
            unimplemented_features.extend(parsed.unimplemented);
        } else if a == "-Xasync" {
            xasync = true;
        } else if a == "-no-specialization" || a == "--no-specialization" {
            no_specialization = true;
        } else if a == "-Ykind-projector" || a == "--kind-projector" {
            kind_projector = true;
        } else if let Some(rest) = a.strip_prefix("-Xsource:") {
            (xsource3, xsource_cross) = parse_xsource_level(rest)?;
        } else if a == "-Xsource" {
            i += 1;
            let ver = args
                .get(i)
                .ok_or_else(|| "option -Xsource requires a version argument".to_string())?;
            (xsource3, xsource_cross) = parse_xsource_level(ver)?;
        } else if a == "-language" {
            i += 1;
            let feats = args
                .get(i)
                .ok_or_else(|| "option -language requires a feature argument".to_string())?;
            for feat in feats.split(',') {
                let f = feat.trim();
                if !f.is_empty() {
                    language_features.push(f.to_string());
                }
            }
        } else if a.starts_with('-') {
            return Err(format!("unknown option '{a}'"));
        } else {
            files.push(PathBuf::from(a));
        }
        i += 1;
    }
    let resolved = if no_scala_library {
        None
    } else {
        match &scala_library {
            Some(p) if p.as_os_str().is_empty() => {
                Some(find_scala_library().ok_or_else(|| {
                    "could not find scala-library 2.13 jar (pass --scala-library <jar> or set SCALA_LIBRARY_JAR)".to_string()
                })?)
            }
            Some(p) => Some(p.clone()),
            None => find_scala_library(),
        }
    };
    let (source_features, warnings) = reconcile_source_features(
        xsource3,
        xsource_cross,
        named_features,
        named_features_given,
        &unimplemented_features,
    );
    Ok(CompileArgs {
        files,
        opts: CompileOptions {
            out_dir,
            parse_only,
            typer_dump,
            fatal_warnings,
            deprecation,
            feature,
            nowarn,
            scala_library: resolved,
            class_path: class_path,
            language_features,
            xsource3,
            source_features,
            xasync,
            no_specialization,
            kind_projector,
        },
        warnings,
        features_help,
        scalac_diagnostics,
    })
}

/// `-Xsource:<version>`. Returns `(source3, cross)`: `source3` is true when
/// the level enables Scala 3 syntax, `cross` when the level is `3-cross`,
/// which nsc defines as `-Xsource:3 -Xsource-features:_` (the post-set hook of
/// `ScalaSettings.source` calls `XsourceFeatures.tryToSet(List("_"))`).
/// nsc refuses anything below the current major version.
fn parse_xsource_level(ver: &str) -> Result<(bool, bool), String> {
    match ver.trim() {
        "" => Err("option -Xsource: requires a version".into()),
        "3" => Ok((true, false)),
        "3-cross" => Ok((true, true)),
        "2.13" | "2.13.0" => Ok((false, false)),
        other => Err(format!(
            "-Xsource must be at least the current major version (2.13.0), got '{other}'"
        )),
    }
}

/// nsc's `ScalaSettings.conflictWarning`: `-Xsource-features` is gated on
/// `isScala3`, so below `-Xsource:3` the whole setting is dropped.
const XSOURCE_FEATURES_CONFLICT: &str = "Conflicting compiler settings were detected. \
Some settings will be ignored.\n-Xsource-features requires -Xsource:3";

/// Reconcile `-Xsource` with `-Xsource-features`, exactly as nsc does.
///
/// `cross` (`-Xsource:3-cross`) turns on every feature; naming features
/// without `-Xsource:3` drops them with a warning. Returns the settings that
/// survive plus the warnings to print.
fn reconcile_source_features(
    source3: bool,
    cross: bool,
    named: SourceFeatures,
    named_given: bool,
    unimplemented: &[&'static str],
) -> (SourceFeatures, Vec<String>) {
    let mut warnings = Vec::new();
    let mut features = named;
    if cross {
        features = SourceFeatures::all();
    }
    if named_given && !source3 {
        warnings.push(XSOURCE_FEATURES_CONFLICT.to_string());
        features = SourceFeatures::default();
    }
    if !features.is_empty() {
        for f in unimplemented {
            if *f == "infer-override" {
                warnings.push("-Xsource-features:infer-override is partially implemented; ordinary methods and fields are supported, macro exceptions remain unverified (see docs/not-implemented.md)".into());
                continue;
            }

            warnings.push(format!(
                "-Xsource-features:{f} is accepted but not implemented by scala-rs; \
it changes nothing (see docs/not-implemented.md)"
            ));
        }
    }
    (features, warnings)
}

fn take_scala_library_flag(args: &[String], i: &mut usize) -> Result<PathBuf, String> {
    let a = args[*i].as_str();
    if let Some(jar) = a.strip_prefix("--scala-library=") {
        if jar.is_empty() {
            return Ok(PathBuf::new());
        }
        return Ok(PathBuf::from(jar));
    }
    let next = args.get(*i + 1).map(|s| s.as_str());
    if let Some(n) = next {
        if n.ends_with(".jar") || (std::path::Path::new(n).is_file() && !n.ends_with(".scala")) {
            *i += 1;
            return Ok(PathBuf::from(n));
        }
    }
    Ok(PathBuf::new())
}

fn split_classpath(s: &str) -> Vec<PathBuf> {
    s.split(':')
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn cmd_run(args: &[String]) -> ExitCode {
    let parsed = match parse_run_args(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(1);
        }
    };

    let out_dir = match make_temp_out() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot create temp directory: {e}");
            return ExitCode::from(1);
        }
    };

    let scala_library = parsed.scala_library;
    let opts = CompileOptions {
        out_dir: out_dir.clone(),
        parse_only: false,
        typer_dump: false,
        fatal_warnings: false,
        deprecation: false,
        feature: false,
        nowarn: false,
        scala_library: scala_library.clone(),
        class_path: Vec::new(),
        language_features: Vec::new(),
        xsource3: parsed.xsource3,
        source_features: parsed.source_features,
        xasync: parsed.xasync,
        no_specialization: parsed.no_specialization,
        kind_projector: parsed.kind_projector,
    };
    let result = compile_paths(&[parsed.file], &opts);
    print_diags(&result);
    if !result.ok() {
        let _ = std::fs::remove_dir_all(&out_dir);
        return ExitCode::from(1);
    }

    let main = result.mains.first().map(String::as_str).unwrap_or("Main");
    let mut extra: Vec<PathBuf> = scala_library.into_iter().collect();
    if let Some(xml) = find_scala_xml() {
        extra.push(xml);
    }

    let output = match run_main_with_cp(&out_dir, &extra, main, &parsed.java_args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: failed to run java: {e}");
            let _ = std::fs::remove_dir_all(&out_dir);
            return ExitCode::from(1);
        }
    };

    let _ = std::io::stdout().write_all(&output.stdout);
    let _ = std::io::stderr().write_all(&output.stderr);
    let _ = std::fs::remove_dir_all(&out_dir);

    match output.status.code() {
        Some(code) => ExitCode::from(code as u8),
        None => ExitCode::from(1),
    }
}

struct RunArgs {
    file: PathBuf,
    java_args: Vec<String>,
    scala_library: Option<PathBuf>,
    xsource3: bool,
    source_features: SourceFeatures,
    xasync: bool,
    no_specialization: bool,
    kind_projector: bool,
}

fn parse_run_args(args: &[String]) -> Result<RunArgs, String> {
    let mut file: Option<PathBuf> = None;
    let mut java_args = Vec::new();
    let mut scala_library = None;
    let mut no_scala_library = false;
    let mut xsource3 = false;
    let mut xsource_cross = false;
    let mut named_features = SourceFeatures::default();
    let mut named_features_given = false;
    let mut xasync = false;
    let mut no_specialization = false;
    let mut kind_projector = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            java_args.extend_from_slice(&args[i + 1..]);
            break;
        } else if let Some(rest) = a.strip_prefix("-Xsource-features:") {
            named_features = SourceFeatures::parse(rest)?.features;
            named_features_given = true;
        } else if a == "-Xasync" {
            xasync = true;
        } else if a == "-no-specialization" || a == "--no-specialization" {
            no_specialization = true;
        } else if a == "-Ykind-projector" || a == "--kind-projector" {
            kind_projector = true;
        } else if let Some(rest) = a.strip_prefix("-Xsource:") {
            (xsource3, xsource_cross) = parse_xsource_level(rest)?;
        } else if a == "--no-scala-library" {
            no_scala_library = true;
        } else if a == "--scala-library" || a.starts_with("--scala-library=") {
            scala_library = Some(take_scala_library_flag(args, &mut i)?);
        } else if file.is_none() {
            if a.starts_with('-') {
                return Err(format!("unknown option '{a}'"));
            }
            file = Some(PathBuf::from(a));
        } else {
            java_args.push(a.to_string());
        }
        i += 1;
    }
    let file = file.ok_or_else(|| "run requires a source file".to_string())?;
    let scala_library = if no_scala_library {
        None
    } else {
        match scala_library {
            Some(p) if p.as_os_str().is_empty() => {
                Some(find_scala_library().ok_or_else(|| {
                    "could not find scala-library 2.13 jar (pass --scala-library <jar> or set SCALA_LIBRARY_JAR)".to_string()
                })?)
            }
            Some(p) => Some(p),
            None => find_scala_library(),
        }
    };
    let (source_features, warnings) = reconcile_source_features(
        xsource3,
        xsource_cross,
        named_features,
        named_features_given,
        &[],
    );
    for w in warnings {
        eprintln!("warning: {w}");
    }
    Ok(RunArgs {
        file,
        java_args,
        scala_library,
        source_features,
        xasync,
        xsource3,
        no_specialization,
        kind_projector,
    })
}

fn make_temp_out() -> std::io::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!("scala-rs-run-{}-{}", std::process::id(), nanos));
    std::fs::create_dir_all(&p)?;
    Ok(p)
}
