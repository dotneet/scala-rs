#!/bin/zsh
# Lightweight regression checks for fixture manifest and artifact-cache rules.
set -e
setopt NO_BG_NICE
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-fixture-cache-test.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

export ROOT
export SCALA_RS_FIXTURE_ROOT=$WORK/fixtures
export FIXTURE_MANIFEST=$ROOT/tests/fixture_manifest.toml
source "$ROOT/tests/fixture_cache.sh"

[[ $(fixture_cfg cats revision) == 32a50dcfad9d897459bb755c4b5a22b4c7bc745c ]] || exit 1
[[ $(fixture_cfg slick source_count) == 184 ]] || exit 1
fixture_validate_source_count cats 340 default
fixture_validate_generated scalalib 0 none

# The toolchain is described once in the manifest.  Relative entries resolve
# below the configurable toolchain root, while absolute entries remain usable
# for private manifests.  Keep this separate from the fixture root: the
# production default is still /tmp, not a cache-specific subdirectory.
TOOLCHAIN_MANIFEST=$WORK/toolchain-manifest.toml
print -r -- '[toolchain]' > "$TOOLCHAIN_MANIFEST"
print -r -- 'scala_home = "scala-home"' >> "$TOOLCHAIN_MANIFEST"
print -r -- 'scala_library = "scala-lib.jar"' >> "$TOOLCHAIN_MANIFEST"
print -r -- 'scala_launcher_library_jar = "scala-home/lib/scala-library.jar"' >> "$TOOLCHAIN_MANIFEST"
print -r -- 'scala_compiler_jar = "scala-home/lib/scala-compiler.jar"' >> "$TOOLCHAIN_MANIFEST"
print -r -- 'scala_reflect_jar = "scala-home/lib/scala-reflect.jar"' >> "$TOOLCHAIN_MANIFEST"
print -r -- 'scalac_launcher = "scala-home/bin/scalac"' >> "$TOOLCHAIN_MANIFEST"
FIXTURE_CACHE_MANIFEST=$TOOLCHAIN_MANIFEST
SCALA_RS_TOOLCHAIN_ROOT=$WORK/toolchain-root
export FIXTURE_CACHE_MANIFEST SCALA_RS_TOOLCHAIN_ROOT
[[ $(fixture_toolchain_home) == "$WORK/toolchain-root/scala-home" ]] || exit 1
[[ $(fixture_toolchain_library) == "$WORK/toolchain-root/scala-lib.jar" ]] || exit 1
[[ $(fixture_toolchain_path scala_library_jar) == "$WORK/toolchain-root/scala-lib.jar" ]] || exit 1
[[ $(fixture_toolchain_launcher_library) == "$WORK/toolchain-root/scala-home/lib/scala-library.jar" ]] || exit 1
[[ $(fixture_scalac_path) == "$WORK/toolchain-root/scala-home/bin/scalac" ]] || exit 1
FIXTURE_CACHE_MANIFEST=$ROOT/tests/fixture_manifest.toml
export FIXTURE_CACHE_MANIFEST
unset SCALA_RS_TOOLCHAIN_ROOT
[[ $(fixture_toolchain_home) == /tmp/scala-2.13.16 ]] || exit 1
[[ $(fixture_toolchain_library) == /tmp/scala-rs-lib/scala-library-2.13.16.jar ]] || exit 1

# Explicit JAVA/JAVAC paths win over JAVA_HOME; with neither explicit path,
# both helpers select the same JDK home used by the fingerprint.
mkdir -p "$WORK/java-home/bin"
for tool in java javac; do
  print -r -- '#!/bin/sh' > "$WORK/java-home/bin/$tool"
  print -r -- 'echo fixture-home >&2' >> "$WORK/java-home/bin/$tool"
  chmod +x "$WORK/java-home/bin/$tool"
done
print -r -- '#!/bin/sh' > "$WORK/java-explicit"
print -r -- 'echo fixture-explicit >&2' >> "$WORK/java-explicit"
chmod +x "$WORK/java-explicit"
print -r -- '#!/bin/sh' > "$WORK/javac-explicit"
print -r -- 'exit 0' >> "$WORK/javac-explicit"
chmod +x "$WORK/javac-explicit"
# `fixture_prepare_toolchain` uses the manifest version to find source jars and
# writes a launcher whose embedded JVM is the same explicit JAVA selected by
# the runtime helpers.
PREP_MANIFEST=$WORK/prep-manifest.toml
print -r -- '[toolchain]' > "$PREP_MANIFEST"
print -r -- 'scala_version = "2.13.16"' >> "$PREP_MANIFEST"
print -r -- 'scala_home = "scala-home"' >> "$PREP_MANIFEST"
print -r -- 'scala_library = "scala-rs-lib/scala-library-2.13.16.jar"' >> "$PREP_MANIFEST"
print -r -- 'scala_launcher_library_jar = "scala-home/lib/scala-library.jar"' >> "$PREP_MANIFEST"
print -r -- 'scala_compiler_jar = "scala-home/lib/scala-compiler.jar"' >> "$PREP_MANIFEST"
print -r -- 'scala_reflect_jar = "scala-home/lib/scala-reflect.jar"' >> "$PREP_MANIFEST"
print -r -- 'scalac_launcher = "scala-home/bin/scalac"' >> "$PREP_MANIFEST"
PREP_CACHE=$WORK/coursier/org/scala-lang
mkdir -p "$PREP_CACHE"/scala-library/2.13.16 "$PREP_CACHE"/scala-compiler/2.13.16 "$PREP_CACHE"/scala-reflect/2.13.16
print -r -- library > "$PREP_CACHE/scala-library/2.13.16/scala-library-2.13.16.jar"
print -r -- compiler > "$PREP_CACHE/scala-compiler/2.13.16/scala-compiler-2.13.16.jar"
print -r -- reflect > "$PREP_CACHE/scala-reflect/2.13.16/scala-reflect-2.13.16.jar"
JAVA_HOME=$WORK/java-home
JAVA=$WORK/java-explicit
JAVAC=$WORK/javac-explicit
export JAVA_HOME JAVA JAVAC
[[ $(fixture_java_path) == "$WORK/java-explicit" ]] || exit 1
[[ $(fixture_javac_path) == "$WORK/javac-explicit" ]] || exit 1
[[ $(fixture_jdk_fingerprint) == *'fixture-explicit'* ]] || exit 1
FIXTURE_CACHE_MANIFEST=$PREP_MANIFEST
SCALA_RS_TOOLCHAIN_ROOT="$WORK/prep root"
export FIXTURE_CACHE_MANIFEST SCALA_RS_TOOLCHAIN_ROOT
unset SCALAC
fixture_prepare_toolchain "$WORK/coursier"
PREP_SCALAC=$(fixture_scalac_path)
[[ -x $PREP_SCALAC ]] || exit 1
grep -Fqx "JAVA=$WORK/java-explicit" "$PREP_SCALAC" || exit 1
[[ $("$PREP_SCALAC" -version 2>&1) == fixture-explicit ]] || exit 1

# A stale prefix is not a valid materialized JAR and is repaired from the
# source. The helper compares both size and digest before accepting a target.
PREP_LIBRARY_TARGET=$(fixture_toolchain_library)
PREP_LIBRARY_SOURCE=$PREP_CACHE/scala-library/2.13.16/scala-library-2.13.16.jar
print -r -- stale-prefix > "$PREP_LIBRARY_TARGET"
fixture_prepare_toolchain "$WORK/coursier"
cmp -s "$PREP_LIBRARY_SOURCE" "$PREP_LIBRARY_TARGET" || {
  print -u2 'stale toolchain JAR was accepted instead of repaired'; exit 1
}

# Two first-use preparations may run at once. Every final JAR must equal the
# source; each writer uses a private same-directory temporary and atomic rename.
PREP_TARGETS=(
  "$PREP_LIBRARY_TARGET"
  "$(fixture_toolchain_launcher_library)"
  "$(fixture_toolchain_compiler)"
  "$(fixture_toolchain_reflect)"
)
PREP_SOURCES=(
  "$PREP_LIBRARY_SOURCE"
  "$PREP_LIBRARY_SOURCE"
  "$PREP_CACHE/scala-compiler/2.13.16/scala-compiler-2.13.16.jar"
  "$PREP_CACHE/scala-reflect/2.13.16/scala-reflect-2.13.16.jar"
)
for target in "${PREP_TARGETS[@]}"; do rm -f "$target"; done
( fixture_prepare_toolchain "$WORK/coursier" ) > "$WORK/prep-one.log" 2>&1 &
PREP_PID_ONE=$!
( fixture_prepare_toolchain "$WORK/coursier" ) > "$WORK/prep-two.log" 2>&1 &
PREP_PID_TWO=$!
PREP_STATUS_ONE=0
PREP_STATUS_TWO=0
wait "$PREP_PID_ONE" || PREP_STATUS_ONE=$?
wait "$PREP_PID_TWO" || PREP_STATUS_TWO=$?
(( PREP_STATUS_ONE == 0 && PREP_STATUS_TWO == 0 )) || {
  print -u2 'concurrent toolchain preparation failed'
  cat "$WORK/prep-one.log" "$WORK/prep-two.log" >&2
  exit 1
}
for i in {1..4}; do
  cmp -s "${PREP_SOURCES[$i]}" "${PREP_TARGETS[$i]}" || {
    print -u2 "concurrent toolchain preparation published a partial JAR: ${PREP_TARGETS[$i]}"
    exit 1
  }
done

# A failed copy leaves no partial final target. The next invocation can safely
# retry using the source and the same atomic publication path.
PREP_FAIL_BIN=$WORK/prep-fail-bin
mkdir -p "$PREP_FAIL_BIN"
PREP_REAL_CP=$(command -v cp)
print -r -- '#!/bin/zsh' > "$PREP_FAIL_BIN/cp"
print -r -- '"$FIXTURE_TEST_REAL_CP" "$@" || exit $?' >> "$PREP_FAIL_BIN/cp"
print -r -- 'if [[ $2 == *.tmp.* ]]; then exit 130; fi' >> "$PREP_FAIL_BIN/cp"
print -r -- 'exit 0' >> "$PREP_FAIL_BIN/cp"
chmod +x "$PREP_FAIL_BIN/cp"
for target in "${PREP_TARGETS[@]}"; do rm -f "$target"; done
OLD_PREP_PATH=$PATH
PATH="$PREP_FAIL_BIN:$PATH"
export FIXTURE_TEST_REAL_CP=$PREP_REAL_CP
rehash
if fixture_prepare_toolchain "$WORK/coursier" > "$WORK/prep-failed.log" 2>&1; then
  print -u2 'interrupted toolchain preparation unexpectedly succeeded'; exit 1
fi
for target in "${PREP_TARGETS[@]}"; do
  [[ ! -e $target ]] || {
    print -u2 "failed toolchain preparation exposed a final JAR: $target"; exit 1
  }
done
PATH=$OLD_PREP_PATH
rehash
unset FIXTURE_TEST_REAL_CP
fixture_prepare_toolchain "$WORK/coursier"
FIXTURE_CACHE_MANIFEST=$ROOT/tests/fixture_manifest.toml
export FIXTURE_CACHE_MANIFEST
unset SCALA_RS_TOOLCHAIN_ROOT
unset JAVA JAVAC
[[ $(fixture_java_path) == "$WORK/java-home/bin/java" ]] || exit 1
[[ $(fixture_javac_path) == "$WORK/java-home/bin/javac" ]] || exit 1
[[ $(fixture_jdk_fingerprint) == *'fixture-home'* ]] || exit 1
unset JAVA_HOME

[[ $(fixture_argument_vector_digest) != $(fixture_argument_vector_digest "") ]] || {
  print -u2 'empty argument vector collided with one empty argument'; exit 1
}
[[ $(fixture_argument_vector_digest "a b" c) != $(fixture_argument_vector_digest a "b c") ]] || {
  print -u2 'argument boundaries collided'; exit 1
}

# Cleanup and lock paths are normalized before checking containment.  This
# rejects lexical traversal and symlink escapes, while allowing a new leaf
# below a root whose parents already exist.
PATH_ROOT=$WORK/fixtures/path-root
mkdir -p "$PATH_ROOT"
if ! fixture_path_is_child "$PATH_ROOT" "$PATH_ROOT/new/leaf"; then
  print -u2 'new leaf below fixture root was rejected'; exit 1
fi
if fixture_path_is_child "$PATH_ROOT" "$PATH_ROOT/../outside"; then
  print -u2 'lexical parent traversal escaped fixture root'; exit 1
fi
if fixture_path_is_child "$PATH_ROOT" "$PATH_ROOT-sibling/leaf"; then
  print -u2 'prefix-only sibling path was accepted'; exit 1
fi
mkdir -p "$WORK/outside"
ln -s "$WORK/outside" "$PATH_ROOT/link-out"
if fixture_path_is_child "$PATH_ROOT" "$PATH_ROOT/link-out/new-leaf"; then
  print -u2 'symlink escape was accepted'; exit 1
fi

# A claimant killed between mkdir and its pid write leaves an empty lock.  It
# remains protected during the short grace, then is reclaimed atomically only
# while it is still empty.  A live pid owner is never reclaimed.
EMPTY_LOCK=$PATH_ROOT/empty.lock
mkdir "$EMPTY_LOCK"
if fixture_lock_acquire "$EMPTY_LOCK" "$PATH_ROOT"; then
  print -u2 'fresh empty lock was stolen before its grace period'; exit 1
fi
export SCALA_RS_LOCK_EMPTY_GRACE=1
sleep 1.1
fixture_lock_acquire "$EMPTY_LOCK" "$PATH_ROOT"
fixture_lock_release "$EMPTY_LOCK" "$PATH_ROOT"
LIVE_LOCK=$PATH_ROOT/live.lock
mkdir "$LIVE_LOCK"
print -r -- $$ > "$LIVE_LOCK/pid"
if fixture_lock_acquire "$LIVE_LOCK" "$PATH_ROOT"; then
  print -u2 'live lock owner was stolen'; exit 1
fi
fixture_lock_release "$LIVE_LOCK" "$PATH_ROOT"
unset SCALA_RS_LOCK_EMPTY_GRACE

mkdir -p "$WORK/source" "$WORK/cp"
print -r -- 'object Fixture' > "$WORK/source/Fixture.scala"
print -r -- 'jar-bytes' > "$WORK/cp/example.jar"
print -r -- 'compiler' > "$WORK/compiler"
print -r -- 'scala compiler' > "$WORK/compiler.jar"
print -r -- 'scala library' > "$WORK/library.jar"
print -r -- 'launcher library' > "$WORK/launcher-library.jar"
print -r -- 'scala reflect' > "$WORK/reflect.jar"
chmod +x "$WORK/compiler"
export SCALAC=$WORK/compiler
export SCALA_COMPILER_JAR=$WORK/compiler.jar
export SCALA_LIBRARY_JAR=$WORK/library.jar
export SCALA_LAUNCHER_LIBRARY_JAR=$WORK/launcher-library.jar
export SCALA_REFLECT_JAR=$WORK/reflect.jar
SOURCE_DIGEST=$(fixture_source_digest "$WORK" "$WORK/source/Fixture.scala")

# Digest records retain the old path/byte semantics while treating spaces as
# argv data rather than text to be split.  Generated paths are normalized so
# private staging roots still produce the same semantic digest.
mkdir -p "$WORK/source/private path/generated/nested dir" "$WORK/digest-output/nested dir" "$WORK/classpath dir/classes"
print -r -- 'object SpaceSource' > "$WORK/source/private path/generated/nested dir/Space Source.scala"
print -r -- 'object SpaceOutput' > "$WORK/digest-output/nested dir/Space Output.class"
print -r -- 'space-class' > "$WORK/classpath dir/classes/Space Class.class"
SPACE_SOURCE_DIGEST=$(fixture_source_digest "$WORK" "$WORK/source/private path/generated/nested dir/Space Source.scala")
SPACE_GENERATED_DIGEST=$(fixture_generated_content_digest "$WORK" "$WORK/source/private path")
[[ $SPACE_SOURCE_DIGEST == $SPACE_GENERATED_DIGEST ]] || {
  print -u2 'generated path normalization changed the digest semantics'; exit 1
}
SPACE_OUTPUT_DIGEST=$(fixture_output_digest "$WORK/digest-output")
SPACE_CP_DIGEST=$(fixture_classpath_digest "$WORK/classpath dir:$WORK/digest-output")
print -r -- 'object ChangedSource' > "$WORK/source/private path/generated/nested dir/Space Source.scala"
[[ $(fixture_source_digest "$WORK" "$WORK/source/private path/generated/nested dir/Space Source.scala") != $SPACE_SOURCE_DIGEST ]] || {
  print -u2 'source byte tampering was not reflected in the digest'; exit 1
}
print -r -- 'object SpaceSource' > "$WORK/source/private path/generated/nested dir/Space Source.scala"
if fixture_source_digest "$WORK" "$WORK/source/private path/generated/nested dir/missing.scala"; then
  print -u2 'missing source was accepted by the digest helper'; exit 1
fi
chmod 000 "$WORK/source/private path/generated/nested dir/Space Source.scala"
if fixture_source_digest "$WORK" "$WORK/source/private path/generated/nested dir/Space Source.scala"; then
  print -u2 'unreadable source was accepted by the digest helper'; exit 1
fi
chmod 644 "$WORK/source/private path/generated/nested dir/Space Source.scala"

# The digest helper owns all per-file hashing.  A shim proves that the hot
# paths do not spawn shasum once per file (or at all) as the tree grows.
mkdir -p "$WORK/digest-bin"
print -r -- '#!/bin/sh' > "$WORK/digest-bin/shasum"
print -r -- 'print=1' >> "$WORK/digest-bin/shasum"
print -r -- 'printf "%s\\n" "$print" >> "$SHASUM_CALL_LOG"' >> "$WORK/digest-bin/shasum"
chmod +x "$WORK/digest-bin/shasum"
export SHASUM_CALL_LOG=$WORK/shasum-calls
: > "$SHASUM_CALL_LOG"
OLD_PATH=$PATH
PATH="$WORK/digest-bin:$PATH"
[[ $(fixture_source_digest "$WORK" "$WORK/source/private path/generated/nested dir/Space Source.scala") == $SPACE_SOURCE_DIGEST ]] || exit 1
[[ $(fixture_generated_content_digest "$WORK" "$WORK/source/private path") == $SPACE_GENERATED_DIGEST ]] || exit 1
[[ $(fixture_output_digest "$WORK/digest-output") == $SPACE_OUTPUT_DIGEST ]] || exit 1
[[ $(fixture_classpath_digest "$WORK/classpath dir:$WORK/digest-output") == $SPACE_CP_DIGEST ]] || exit 1
PATH=$OLD_PATH
[[ ! -s $SHASUM_CALL_LOG ]] || {
  print -u2 'fixture digest hot paths still invoke shasum per file'; exit 1
}

CP="$WORK/cp/example.jar:$WORK/library.jar:$WORK/reflect.jar"
fixture_validate_classpath "$CP"
KEY=$(fixture_artifact_key fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-flag' "$CP")
DIR=$(fixture_cache_path fixture scala-rs "$KEY")
STAGE=$(fixture_artifact_stage fixture scala-rs "$KEY")
mkdir -p "$STAGE/out"
: > "$STAGE/out/Fixture.class"
JDK_FINGERPRINT=$(fixture_jdk_fingerprint) || exit 1
fixture_artifact_publish "$STAGE" "$DIR" "$KEY" \
  'fixture=fixture' 'kind=scala-rs' 'source_count=1' "compiler_hash=$(fixture_file_hash "$WORK/compiler")" \
  'upstream_revision=revision' "source_digest=$SOURCE_DIGEST" 'flags=-flag' \
  "classpath_digest=$(fixture_classpath_digest "$CP")" "jdk=$JDK_FINGERPRINT" \
  "scala_toolchain=$(fixture_toolchain_digest)" \
  "scalac=$WORK/compiler" "scalac_hash=$(fixture_file_hash "$WORK/compiler")"
fixture_artifact_valid "$DIR" "$KEY" 1 fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-flag' "$CP"

# A jdk= line is not sufficient: hand-editing it to describe another JDK must
# invalidate the artifact even when the opaque artifact key is unchanged.
cp "$DIR/manifest" "$WORK/manifest-before-jdk-edit"
sed 's/^jdk=.*/jdk=path=another-jdk version=another/' "$WORK/manifest-before-jdk-edit" > "$DIR/manifest"
if fixture_artifact_valid "$DIR" "$KEY" 1 fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-flag' "$CP"; then
  print -u2 'hand-edited/different-JDK manifest was accepted'
  exit 1
fi
mv -f "$WORK/manifest-before-jdk-edit" "$DIR/manifest"
fixture_artifact_valid "$DIR" "$KEY" 1 fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-flag' "$CP"

# Concurrent publishers build in private directories and both report success;
# the first complete destination remains authoritative for the key.
RACE_KEY=$(fixture_artifact_key fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-race' "$CP")
RACE_DIR=$(fixture_cache_path fixture scala-rs "$RACE_KEY")
RACE_ONE=$(fixture_artifact_stage fixture scala-rs "$RACE_KEY")
RACE_TWO=$(fixture_artifact_stage fixture scala-rs "$RACE_KEY")
mkdir -p "$RACE_ONE/out" "$RACE_TWO/out"
print -r -- first > "$RACE_ONE/out/First.class"
print -r -- second > "$RACE_TWO/out/Second.class"
RACE_METADATA=(
  'fixture=fixture' 'kind=scala-rs' 'source_count=1'
  "compiler_hash=$(fixture_file_hash "$WORK/compiler")"
  'upstream_revision=revision' "source_digest=$SOURCE_DIGEST" 'flags=-race'
  "classpath_digest=$(fixture_classpath_digest "$CP")" "jdk=$(fixture_jdk_fingerprint)"
  "scala_toolchain=$(fixture_toolchain_digest)" "scalac=$WORK/compiler"
  "scalac_hash=$(fixture_file_hash "$WORK/compiler")"
)
( fixture_artifact_publish "$RACE_ONE" "$RACE_DIR" "$RACE_KEY" "${RACE_METADATA[@]}" ) > "$WORK/race-one.log" 2>&1 &
RACE_PID_ONE=$!
( fixture_artifact_publish "$RACE_TWO" "$RACE_DIR" "$RACE_KEY" "${RACE_METADATA[@]}" ) > "$WORK/race-two.log" 2>&1 &
RACE_PID_TWO=$!
RACE_STATUS_ONE=0
RACE_STATUS_TWO=0
wait "$RACE_PID_ONE" || RACE_STATUS_ONE=$?
wait "$RACE_PID_TWO" || RACE_STATUS_TWO=$?
(( RACE_STATUS_ONE == 0 && RACE_STATUS_TWO == 0 )) || {
  print -u2 'concurrent artifact publishers did not both succeed'
  cat "$WORK/race-one.log" "$WORK/race-two.log" >&2
  exit 1
}
[[ -f "$RACE_DIR/out/First.class" || -f "$RACE_DIR/out/Second.class" ]] || {
  print -u2 'concurrent artifact publication lost the complete output'; exit 1
}
[[ -f "$RACE_DIR/COMPLETE" ]] || {
  print -u2 'concurrent artifact publication exposed an incomplete output'; exit 1
}
RACE_THREE=$(fixture_artifact_stage fixture scala-rs "$RACE_KEY")
mkdir -p "$RACE_THREE/out"
print -r -- replacement > "$RACE_THREE/out/Replacement.class"
fixture_artifact_publish "$RACE_THREE" "$RACE_DIR" "$RACE_KEY" "${RACE_METADATA[@]}"
[[ ! -e "$RACE_DIR/out/Replacement.class" ]] || {
  print -u2 'later complete publisher replaced the first artifact'; exit 1
}
fixture_artifact_valid "$RACE_DIR" "$RACE_KEY" 1 fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-race' "$CP"

# COMPLETE alone is not sufficient: corrupting the manifest and output is
# repaired by a later valid stage while the publish lock is held.
if [[ -f "$RACE_DIR/out/First.class" ]]; then
  print -r -- corrupted > "$RACE_DIR/out/First.class"
else
  print -r -- corrupted > "$RACE_DIR/out/Second.class"
fi
sed 's/^artifact_key=.*/artifact_key=corrupted/' "$RACE_DIR/manifest" > "$RACE_DIR/manifest.corrupt"
mv -f "$RACE_DIR/manifest.corrupt" "$RACE_DIR/manifest"
RACE_REPAIR=$(fixture_artifact_stage fixture scala-rs "$RACE_KEY")
mkdir -p "$RACE_REPAIR/out"
print -r -- repaired > "$RACE_REPAIR/out/Repaired.class"
fixture_artifact_publish "$RACE_REPAIR" "$RACE_DIR" "$RACE_KEY" "${RACE_METADATA[@]}"
[[ -f "$RACE_DIR/out/Repaired.class" ]] || {
  print -u2 'corrupted complete artifact was not repaired'; exit 1
}
fixture_artifact_valid "$RACE_DIR" "$RACE_KEY" 1 fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-race' "$CP"

# The manifest's source count and output content are part of validity.
if fixture_artifact_valid "$DIR" "$KEY" 2 fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-flag' "$CP"; then
  print -u2 'source-count mismatch was accepted'
  exit 1
fi
print -r -- 'stale output' > "$DIR/out/stale.txt"
if fixture_artifact_valid "$DIR" "$KEY" 1 fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-flag' "$CP"; then
  print -u2 'stale artifact output was accepted'
  exit 1
fi
rm "$DIR/out/stale.txt"

# Changing a toolchain/compiler byte invalidates the old key and metadata.
print -r -- 'changed compiler' > "$WORK/compiler"
if fixture_artifact_valid "$DIR" "$KEY" 1 fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-flag' "$CP"; then
  print -u2 'changed compiler was accepted'
  exit 1
fi
print -r -- 'compiler' > "$WORK/compiler"

# A completion marker is mandatory; an interrupted publication is never reused.
rm "$DIR/COMPLETE"
if fixture_artifact_valid "$DIR" "$KEY" 1 fixture scala-rs "$WORK/compiler" revision "$SOURCE_DIGEST" '-flag' "$CP"; then
  print -u2 'incomplete artifact was accepted'
  exit 1
fi

# Generated-source markers include output content, so a modified expansion is
# rejected even when its revision/input marker is unchanged.
mkdir -p "$WORK/generated"
print -r -- 'object Generated' > "$WORK/generated/Generated.scala"
GEN_DIGEST=$(fixture_generated_content_digest "$WORK" "$WORK/generated")
fixture_generated_mark fixture "$WORK/generated" revision input 1 generated "$GEN_DIGEST"
fixture_generated_valid fixture "$WORK/generated" revision input 1 generated "$GEN_DIGEST"
print -r -- 'object Changed' > "$WORK/generated/Generated.scala"
CHANGED_GEN_DIGEST=$(fixture_generated_content_digest "$WORK" "$WORK/generated")
if fixture_generated_valid fixture "$WORK/generated" revision input 1 generated "$CHANGED_GEN_DIGEST"; then
  print -u2 'changed generated output was accepted'
  exit 1
fi

# A tracked dirty checkout is rejected even when HEAD remains pinned.
mkdir -p "$WORK/repo"
git -C "$WORK/repo" init -q
git -C "$WORK/repo" config user.email fixture@example.invalid
git -C "$WORK/repo" config user.name fixture-test
print -r -- 'tracked' > "$WORK/repo/tracked.txt"
git -C "$WORK/repo" add tracked.txt
git -C "$WORK/repo" commit -q -m initial
LOCAL_REV=$(git -C "$WORK/repo" rev-parse HEAD)
print -r -- '[fixture]' > "$WORK/local-manifest.toml"
print -r -- "url=\"file://$WORK/repo\"" >> "$WORK/local-manifest.toml"
print -r -- "revision=\"$LOCAL_REV\"" >> "$WORK/local-manifest.toml"
FIXTURE_CACHE_MANIFEST=$WORK/local-manifest.toml
export FIXTURE_CACHE_MANIFEST
CHECKOUT_DIR=$FIXTURE_ROOT/checkout
fixture_require_checkout fixture "$CHECKOUT_DIR"

# Checkout destinations must remain below the fixture root after resolving
# symlinks. These checks happen before any fetch/checkout command is run.
mkdir -p "$WORK/outside-parent"
ln -s "$WORK/outside-parent" "$FIXTURE_ROOT/escape-parent"
if fixture_require_checkout fixture "$FIXTURE_ROOT/escape-parent/checkout"; then
  print -u2 'checkout through an escaping parent symlink was accepted'; exit 1
fi
mkdir -p "$WORK/outside-dir"
ln -s "$WORK/outside-dir" "$FIXTURE_ROOT/escape-dir"
if fixture_require_checkout fixture "$FIXTURE_ROOT/escape-dir"; then
  print -u2 'checkout through an escaping dir symlink was accepted'; exit 1
fi
if fixture_require_checkout fixture "$WORK/outside-checkout"; then
  print -u2 'checkout outside fixture root was accepted'; exit 1
fi

# Revision repair must keep the checkout lock until checkout itself finishes.
# Delay the first repair's checkout so a concurrent reader has a deterministic
# window in which it must fail on the live lock instead of racing the mutation.
print -r -- 'tracked-v2' > "$WORK/repo/tracked.txt"
git -C "$WORK/repo" add tracked.txt
git -C "$WORK/repo" commit -q -m second
LOCAL_REV2=$(git -C "$WORK/repo" rev-parse HEAD)
print -r -- '[fixture]' > "$WORK/local-manifest.toml"
print -r -- "url=\"file://$WORK/repo\"" >> "$WORK/local-manifest.toml"
print -r -- "revision=\"$LOCAL_REV2\"" >> "$WORK/local-manifest.toml"
mkdir -p "$WORK/git-bin"
REAL_GIT=$(command -v git)
CHECKOUT_STARTED=$WORK/checkout-started
print -r -- '#!/bin/zsh' > "$WORK/git-bin/git"
print -r -- 'if [[ $1 == -C && $3 == checkout ]]; then' >> "$WORK/git-bin/git"
print -r -- '  print -r -- started > "$FIXTURE_TEST_CHECKOUT_STARTED"' >> "$WORK/git-bin/git"
print -r -- '  sleep "$FIXTURE_TEST_CHECKOUT_DELAY"' >> "$WORK/git-bin/git"
print -r -- 'fi' >> "$WORK/git-bin/git"
print -r -- 'exec "$FIXTURE_TEST_REAL_GIT" "$@"' >> "$WORK/git-bin/git"
chmod +x "$WORK/git-bin/git"
export FIXTURE_TEST_CHECKOUT_STARTED=$CHECKOUT_STARTED
export FIXTURE_TEST_CHECKOUT_DELAY=1
export FIXTURE_TEST_REAL_GIT=$REAL_GIT
OLD_GIT_PATH=$PATH
PATH="$WORK/git-bin:$PATH"
rehash
( fixture_require_checkout fixture "$CHECKOUT_DIR" ) > "$WORK/checkout-repair.log" 2>&1 &
REPAIR_PID=$!
for attempt in {1..100}; do
  [[ -f $CHECKOUT_STARTED ]] && break
  sleep 0.01
done
[[ -f $CHECKOUT_STARTED ]] || {
  kill "$REPAIR_PID" 2>/dev/null || true
  wait "$REPAIR_PID" 2>/dev/null || true
  print -u2 'checkout repair did not reach delayed checkout'; exit 1
}
if fixture_require_checkout fixture "$CHECKOUT_DIR" > "$WORK/checkout-reader.log" 2>&1; then
  wait "$REPAIR_PID" 2>/dev/null || true
  print -u2 'concurrent checkout reader ran during revision repair'; exit 1
fi
wait "$REPAIR_PID" || {
  cat "$WORK/checkout-repair.log" >&2
  print -u2 'revision repair failed'; exit 1
}
[[ $(git -C "$CHECKOUT_DIR" rev-parse HEAD) == $LOCAL_REV2 ]] || {
  print -u2 'revision repair did not select the manifest revision'; exit 1
}
fixture_require_checkout fixture "$CHECKOUT_DIR"
PATH=$OLD_GIT_PATH
rehash
unset FIXTURE_TEST_CHECKOUT_STARTED FIXTURE_TEST_CHECKOUT_DELAY FIXTURE_TEST_REAL_GIT

print -r -- 'dirty' > "$CHECKOUT_DIR/tracked.txt"
if fixture_require_checkout fixture "$CHECKOUT_DIR"; then
  print -u2 'dirty checkout was accepted'
  exit 1
fi
print 'fixture cache: regression checks passed'
