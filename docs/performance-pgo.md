# Experimental profile-guided builds

Profile-guided optimization (PGO) uses recorded execution frequencies when
optimizing the compiler executable. This is an optional build procedure,
not a change to the Scala language implementation or the default release
profile. See the [Rust PGO guide](https://doc.rust-lang.org/rustc/profile-guided-optimization.html).

## Reproduce with an isolated build directory

Keep the source tree and all non-PGO Rust flags unchanged between the
control, instrumented and optimized builds. Use a matching LLVM toolchain,
a fresh profile directory and representative successful training inputs.
Do not reuse a stale profile after changing the compiler.

From the repository root, with the normal Rust build prerequisites installed:

```sh
rustup component add llvm-tools-preview
pgo_root=$(mktemp -d /tmp/scala-rs-pgo.XXXXXX)
pgo_target=$(rustc -vV | sed -n 's/^host: //p')
pgo_llvm="$(rustc --print sysroot)/lib/rustlib/$pgo_target/bin/llvm-profdata"
mkdir -p "$pgo_root/raw"

CARGO_TARGET_DIR="$pgo_root/target" RUSTFLAGS='' \
  cargo build --release --locked --target "$pgo_target" -p scala-rs-cli
cp "$pgo_root/target/$pgo_target/release/scala-rs" "$pgo_root/control"

CARGO_TARGET_DIR="$pgo_root/target" \
RUSTFLAGS="-Cprofile-generate=$pgo_root/raw" \
  cargo build --release --locked --target "$pgo_target" -p scala-rs-cli
cp "$pgo_root/target/$pgo_target/release/scala-rs" "$pgo_root/instrumented"
export LLVM_PROFILE_FILE="$pgo_root/raw/%m_%p.profraw"
```

Now run representative full compilations with `$pgo_root/instrumented`
and its `compile` subcommand, supplying the usual sources, library, classpath, language options
and fresh output directory. Require successful compilation and unchanged
outputs. Reserve separate modules for validation; do not train on every
workload used to judge generalization. Instrumented compilation times are
not benchmark results. After successful profile collection:

```sh
unset LLVM_PROFILE_FILE
"$pgo_llvm" merge -o "$pgo_root/merged.profdata" "$pgo_root/raw"

CARGO_TARGET_DIR="$pgo_root/target" \
RUSTFLAGS="-Cprofile-use=$pgo_root/merged.profdata -Cllvm-args=-pgo-warn-missing-function" \
  cargo build --release --locked --target "$pgo_target" -p scala-rs-cli
cp "$pgo_root/target/$pgo_target/release/scala-rs" "$pgo_root/optimized"

"$pgo_root/control" --help
"$pgo_root/optimized" --help
```

Explicit `--target` keeps profile instrumentation out of Cargo's host build
scripts. Inspect missing-profile warnings and record the source revision,
working-tree changes, toolchain, profile hash and executable hashes. The
example clears `RUSTFLAGS` for the control; if using custom flags, include
the same flags in all three builds and avoid a conflicting
`CARGO_ENCODED_RUSTFLAGS` override.

Use the alternating, fresh-output comparison described in
[performance.md](performance.md), with both trained and held-out workloads.
Do not overlap compilation benchmarks with other owned builds or tests.
Preserve timeout and protocol protection. Check diagnostics and every output
class byte, not just exit codes. Retain the normal executable while
validating the optimized one; this procedure does not overwrite the
repository's default `target/release/scala-rs`.
