//! Shared support for the CLI's process-backed integration tests.
//!
//! The tests intentionally exercise the `scala-rs` binary instead of calling
//! driver internals.  This module keeps the process and temporary-directory
//! plumbing in one place while leaving each fixture's assertions local.
#![allow(dead_code)]

use std::ops::Deref;
use std::process::{Child, Command, ExitStatus, Output};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    env,
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// A unique temporary directory that removes itself when the test leaves it.
///
/// Keeping the directory alive for the whole test makes output paths easy to
/// pass to child processes, while `Drop` also cleans up after assertion
/// failures and early returns.
#[derive(Debug)]
pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub fn new(label: &str) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let label: String = label
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let path = env::temp_dir().join(format!(
            "scala-rs-test-{label}-{}-{now}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test temporary directory");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Deref for TestDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.path()
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl AsRef<OsStr> for TestDir {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if env::var_os("KEEP_TEST_ARTIFACTS").is_none() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Toolchain {
    java: Option<PathBuf>,
    javac: Option<PathBuf>,
    scalac: Option<PathBuf>,
    scala_library: Option<PathBuf>,
    scala_reflect: Option<PathBuf>,
}

impl Toolchain {
    pub fn java(&self) -> Option<&Path> {
        self.java.as_deref()
    }

    pub fn javac(&self) -> Option<&Path> {
        self.javac.as_deref()
    }

    pub fn scalac(&self) -> Option<&Path> {
        self.scalac.as_deref()
    }

    pub fn scala_library(&self) -> Option<&Path> {
        self.scala_library.as_deref()
    }

    pub fn scala_reflect(&self) -> Option<&Path> {
        self.scala_reflect.as_deref()
    }

    pub fn has_java(&self) -> bool {
        self.java.is_some()
    }
}

static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();

/// Discover external tools once per integration-test process.
pub fn toolchain() -> &'static Toolchain {
    TOOLCHAIN.get_or_init(discover_toolchain)
}

fn discover_toolchain() -> Toolchain {
    let scala_home = env::var_os("SCALA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/scala-2.13.16"));
    Toolchain {
        java: env_executable("JAVA").or_else(|| find_executable("java")),
        javac: env_executable("JAVAC").or_else(|| find_executable("javac")),
        scalac: first_file([
            env::var_os("SCALAC").map(PathBuf::from).unwrap_or_default(),
            scala_home.join("bin/scalac"),
            find_executable("scalac").unwrap_or_default(),
        ]),
        scala_library: first_file([
            env::var_os("SCALA_LIBRARY_JAR")
                .map(PathBuf::from)
                .unwrap_or_default(),
            PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar"),
            scala_home.join("lib/scala-library.jar"),
        ]),
        scala_reflect: first_file([
            env::var_os("SCALA_REFLECT_JAR")
                .map(PathBuf::from)
                .unwrap_or_default(),
            scala_home.join("lib/scala-reflect.jar"),
        ]),
    }
}

fn env_executable(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn first_file<const N: usize>(candidates: [PathBuf; N]) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_file())
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(name);
    if path.components().count() > 1 {
        return path.is_file().then_some(path);
    }
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

pub fn scala_rs() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

#[derive(Debug)]
pub struct CompileOutcome {
    output: Output,
}

impl CompileOutcome {
    pub fn from_output(output: Output) -> Self {
        Self { output }
    }

    pub fn success(&self) -> bool {
        self.output.status.success()
    }

    pub fn status(&self) -> &ExitStatus {
        &self.output.status
    }

    pub fn stdout(&self) -> &[u8] {
        &self.output.stdout
    }

    pub fn stderr(&self) -> &[u8] {
        &self.output.stderr
    }

    pub fn diagnostics(&self) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(self.stdout()),
            String::from_utf8_lossy(self.stderr())
        )
    }

    pub fn assert_success(&self, context: &str) {
        assert!(self.success(), "{context}: {}", self.diagnostics());
    }
}

/// Builder for the common `scala-rs compile ... -d ...` invocation.
#[derive(Debug)]
pub struct CompileCommand {
    command: Command,
}

impl CompileCommand {
    pub fn new(source: impl AsRef<Path>, output: impl AsRef<Path>) -> Self {
        let mut command = Command::new(scala_rs());
        command
            .arg("compile")
            .arg(source.as_ref())
            .arg("-d")
            .arg(output.as_ref());
        Self { command }
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.command.arg(arg);
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(args);
        self
    }

    pub fn classpath(self, classpath: impl AsRef<OsStr>) -> Self {
        self.args([OsString::from("-cp"), classpath.as_ref().to_os_string()])
    }

    pub fn scala_library(self, jar: impl AsRef<OsStr>) -> Self {
        self.args([
            OsString::from("--scala-library"),
            jar.as_ref().to_os_string(),
        ])
    }

    pub fn no_scala_library(self) -> Self {
        self.arg("--no-scala-library")
    }

    pub fn run(mut self) -> CompileOutcome {
        CompileOutcome {
            output: self.command.output().expect("run scala-rs compile"),
        }
    }
}

#[derive(Debug)]
pub struct RunOutcome {
    output: Output,
}

impl RunOutcome {
    pub fn success(&self) -> bool {
        self.output.status.success()
    }

    pub fn status(&self) -> &ExitStatus {
        &self.output.status
    }

    pub fn stdout(&self) -> &[u8] {
        &self.output.stdout
    }

    pub fn stderr(&self) -> &[u8] {
        &self.output.stderr
    }

    pub fn stdout_string(&self) -> String {
        String::from_utf8_lossy(self.stdout()).into_owned()
    }

    pub fn assert_success(&self, context: &str) {
        assert!(
            self.success(),
            "{context}: {}",
            String::from_utf8_lossy(self.stderr())
        );
    }
}

/// Builder for a verified JVM run of a compiled fixture.
#[derive(Debug)]
pub struct RunCommand {
    main: String,
    classpath: OsString,
    args: Vec<OsString>,
}

impl RunCommand {
    pub fn new(main: &str) -> Self {
        Self {
            main: main.to_owned(),
            classpath: OsString::new(),
            args: Vec::new(),
        }
    }

    pub fn classpath(mut self, classpath: impl AsRef<OsStr>) -> Self {
        self.classpath = classpath.as_ref().to_os_string();
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
        self
    }

    pub fn run(self) -> RunOutcome {
        let mut command = Command::new(toolchain().java().unwrap_or_else(|| Path::new("java")));
        command
            .args([OsString::from("-Xverify:all"), OsString::from("-cp")])
            .arg(&self.classpath)
            .arg(&self.main)
            .args(&self.args);
        RunOutcome {
            output: command.output().expect("run java"),
        }
    }
}

/// RAII guard for a directly spawned compiler process. Dropping the guard
/// reaps that child; subprocess trees still require a process-group-aware
/// launcher such as `tests/reap_strays.sh` when the child spawns descendants.
#[derive(Debug)]
pub struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    pub fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .expect("child guard already consumed")
            .try_wait()
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child
            .as_mut()
            .expect("child guard already consumed")
            .wait()
    }

    pub fn wait_with_output(mut self) -> io::Result<Output> {
        self.child
            .take()
            .expect("child guard already consumed")
            .wait_with_output()
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child
            .as_mut()
            .expect("child guard already consumed")
            .kill()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let running = child
            .try_wait()
            .map(|status| status.is_none())
            .unwrap_or(true);
        if running {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
