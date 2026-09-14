#!/bin/zsh
# Shared external-fixture and compiled-artifact support.
#
# This file deliberately contains no checkout-specific or session-specific
# paths.  Source checkouts, generated inputs, and immutable compiled artifacts
# live below SCALA_RS_FIXTURE_ROOT (default: ${TMPDIR:-/tmp}/scala-rs-fixtures).
# A cache entry is usable only when its manifest and completion marker agree,
# the input/revision/toolchain fingerprints match, and it contains classfiles.

if [[ -z ${FIXTURE_CACHE_ROOT_INITIALIZED:-} ]]; then
  FIXTURE_CACHE_ROOT_INITIALIZED=1
  FIXTURE_CACHE_MANIFEST=${FIXTURE_MANIFEST:-${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}/tests/fixture_manifest.toml}
  FIXTURE_ROOT=${SCALA_RS_FIXTURE_ROOT:-${TMPDIR:-/tmp}/scala-rs-fixtures}
  FIXTURE_CACHE=${SCALA_RS_CACHE_ROOT:-$FIXTURE_ROOT/cache}
  FIXTURE_DIGEST_HELPER=${FIXTURE_DIGEST_HELPER:-${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}/tests/fixture_digest.pl}
fi

fixture_path_is_child() {
  local root=$1 target_path=$2 canonical_root canonical_target
  [[ -n $root && -n $target_path ]] || return 1
  # :A canonicalizes existing path components (including symlinks) and
  # normalizes `.`/`..` while still producing an absolute path for a leaf
  # that does not exist yet.  Refuse anything that cannot be canonicalized
  # into two absolute paths before doing the component-boundary check.
  canonical_root=${root:A}
  canonical_target=${target_path:A}
  [[ -n $canonical_root && -n $canonical_target ]] || return 1
  [[ $canonical_root == /* && $canonical_target == /* ]] || return 1
  [[ $canonical_target != $canonical_root && $canonical_target == $canonical_root/* ]]
}

fixture_safe_clean() {
  # Recursive deletion is restricted to a known fixture/cache child.  Callers
  # with explicitly supplied output paths retain the old behavior only when
  # they opt into that path themselves.
  local root=$1 target_path=$2
  fixture_path_is_child "$root" "$target_path" || {
    print -u2 "refusing recursive cleanup outside fixture root: $target_path"
    return 1
  }
  rm -rf -- "$target_path"
}

fixture_lock_acquire() {
  local lock=$1 root=${2:-$FIXTURE_ROOT} owner lock_mtime now empty_grace
  fixture_path_is_child "$root" "$lock" || {
    print -u2 "fixture lock is outside its owner root: $lock"; return 1
  }
  mkdir -p "${lock:h}"
  if mkdir "$lock" 2>/dev/null; then
    print -r -- $$ > "$lock/pid"
    return 0
  fi
  if [[ -r $lock/pid ]]; then
    owner=$(<"$lock/pid")
  else
    owner=
  fi
  if [[ -n $owner ]] && ! kill -0 "$owner" 2>/dev/null; then
    fixture_safe_clean "$root" "$lock"
    mkdir "$lock" || return 1
    print -r -- $$ > "$lock/pid"
    return 0
  fi
  if [[ -z $owner && -d $lock ]]; then
    # mkdir is the claim, but a process can be killed in the tiny window
    # before its pid file is written.  Do not recursively remove this lock:
    # after a short age grace, rmdir succeeds only while it is still empty,
    # so an initializer that has written its pid (or any other state) cannot
    # be stolen by a concurrent claimant.
    empty_grace=${SCALA_RS_LOCK_EMPTY_GRACE:-2}
    [[ $empty_grace == <-> ]] || empty_grace=2
    lock_mtime=$(stat -f %m -- "$lock" 2>/dev/null || true)
    [[ $lock_mtime == <-> ]] || lock_mtime=$(stat -c %Y -- "$lock" 2>/dev/null || true)
    now=$(date +%s 2>/dev/null || true)
    if [[ $lock_mtime == <-> && $now == <-> ]] && (( now - lock_mtime >= empty_grace )); then
      if rmdir "$lock" 2>/dev/null && mkdir "$lock" 2>/dev/null; then
        print -r -- $$ > "$lock/pid"
        return 0
      fi
    fi
  fi
  print -u2 "fixture initialization is already in progress: $lock"
  return 1
}

fixture_lock_release() {
  local lock=$1 root=${2:-$FIXTURE_ROOT}
  if [[ -d $lock ]]; then fixture_safe_clean "$root" "$lock"; fi
  return 0
}

fixture_cfg() {
  local section=$1 key=$2 file=${3:-$FIXTURE_CACHE_MANIFEST}
  [[ -r $file ]] || { print -u2 "fixture manifest is missing: $file"; return 1; }
  awk -v want_section="$section" -v want_key="$key" '
    /^[[:space:]]*\[/ {
      section=$0
      sub(/^[[:space:]]*\[/, "", section)
      sub(/\][[:space:]]*$/, "", section)
      in_section=(section == want_section)
      next
    }
    in_section {
      line=$0
      sub(/[[:space:]]*#.*/, "", line)
      if (line ~ "^[[:space:]]*" want_key "[[:space:]]*=") {
        sub("^[[:space:]]*" want_key "[[:space:]]*=[[:space:]]*", "", line)
        sub(/[[:space:]]+$/, "", line)
        if (line ~ /^".*"$/) { sub(/^"/, "", line); sub(/"$/, "", line) }
        print line
        exit
      }
    }
  ' "$file"
}

fixture_path() {
  local fixture=$1
  print -r -- "$FIXTURE_ROOT/$fixture"
}

fixture_cache_path() {
  local fixture=$1 kind=$2 key=$3
  print -r -- "$FIXTURE_CACHE/$fixture/$kind/$key"
}

fixture_require_checkout() {
  local fixture=$1 dir=$2 url rev actual parent lock stage
  url=$(fixture_cfg "$fixture" url)
  rev=$(fixture_cfg "$fixture" revision)
  [[ -n $url && -n $rev ]] || { print -u2 "fixture manifest entry incomplete: $fixture"; return 1; }
  # A checkout is mutable state.  Resolve the complete destination before
  # creating its parent or invoking git, so an absolute override, a `..`
  # traversal, or a symlink below the fixture root cannot redirect clone,
  # fetch, or checkout to an unrelated tree.  fixture_path_is_child resolves
  # existing components while still allowing the final checkout directory to
  # be created on first use.
  fixture_path_is_child "$FIXTURE_ROOT" "$dir" || {
    print -u2 "fixture checkout is outside fixture root: $dir"
    return 1
  }
  # A final symlink is not a checkout destination we own.  Reject it even
  # when it currently points back inside the root; this also closes the
  # dangling-symlink case where `${dir:A}` can only canonicalize to its parent.
  if [[ -L $dir ]]; then
    print -u2 "fixture checkout destination is a symlink: $dir"
    return 1
  fi
  # The checkout lock covers both first-use initialization and the complete
  # revision validation/repair below.  In particular, do not release it after
  # clone and then fetch/checkout: another fixture consumer could otherwise
  # observe or mutate the same checkout while it is being repaired.
  parent=${dir:h}
  mkdir -p "$parent" || return 1
  lock="$parent/.${fixture}.checkout.lock"
  fixture_lock_acquire "$lock" "$parent" || return 1

  if [[ -e $dir && ! -d $dir/.git ]]; then
    print -u2 "fixture path exists but is not a git checkout: $dir"
    fixture_lock_release "$lock" "$parent"
    return 1
  fi

  if [[ ! -d $dir/.git ]]; then
    # Another process may have completed setup before this process acquired a
    # stale lock.  Recheck before cloning so first-use initialization remains
    # atomic and idempotent.
    stage=$(mktemp -d "$parent/.${fixture}.checkout.XXXXXX") || {
      fixture_lock_release "$lock" "$parent"
      print -u2 "could not create staging directory for $fixture: $parent"
      return 1
    }
    git clone "$url" "$stage/repo" >/dev/null 2>&1 || {
      fixture_safe_clean "$parent" "$stage"
      fixture_lock_release "$lock" "$parent"
      print -u2 "could not clone $fixture from $url"
      return 1
    }
    mv "$stage/repo" "$dir" || {
      fixture_safe_clean "$parent" "$stage"
      fixture_lock_release "$lock" "$parent"
      print -u2 "could not install $fixture checkout: $dir"
      return 1
    }
    rmdir "$stage" 2>/dev/null || true
  fi
  actual=$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)
  if [[ $actual != $rev ]]; then
    # A wrong or shallow checkout is repaired through git's normal fetch and
    # checkout.  If local changes prevent that, fail closed rather than
    # silently measuring a different source tree.
    git -C "$dir" fetch --quiet origin "$rev" || {
      fixture_lock_release "$lock" "$parent"
      print -u2 "cannot fetch $fixture revision $rev"; return 1
    }
    git -C "$dir" checkout -q "$rev" || {
      fixture_lock_release "$lock" "$parent"
      print -u2 "$fixture checkout has local changes or cannot select $rev"; return 1
    }
    actual=$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)
  fi
  if [[ -n $(git -C "$dir" status --porcelain --untracked-files=no 2>/dev/null) ]]; then
    print -u2 "$fixture checkout has tracked local changes: $dir"
    fixture_lock_release "$lock" "$parent"
    return 1
  fi
  if [[ $actual != $rev ]]; then
    fixture_lock_release "$lock" "$parent"
    print -u2 "$fixture revision mismatch: got ${actual:-missing}, expected $rev"; return 1
  fi
  fixture_lock_release "$lock" "$parent"
}

fixture_file_hash() {
  [[ -s $1 ]] || return 1
  shasum -a 256 "$1" | awk '{print $1}'
}

fixture_file_size() {
  local file=$1 size
  [[ -f $file ]] || return 1
  size=$(stat -f %z -- "$file" 2>/dev/null || true)
  [[ $size == <-> ]] || size=$(stat -c %s -- "$file" 2>/dev/null || true)
  [[ $size == <-> ]] || return 1
  print -r -- "$size"
}

fixture_materialized_file_valid() {
  # A non-empty destination is not enough: an interrupted direct copy can
  # leave a plausible-looking prefix behind.  Compare both size and SHA-256
  # with the immutable source before allowing a materialized file to be used.
  local source=$1 target=$2 expected_size=$3 expected_hash=$4 target_size target_hash
  [[ -s $source && -f $source && -s $target && -f $target ]] || return 1
  target_size=$(fixture_file_size "$target") || return 1
  [[ $target_size == $expected_size ]] || return 1
  target_hash=$(fixture_file_hash "$target") || return 1
  [[ $target_hash == $expected_hash ]]
}

fixture_materialize_file() {
  # Copy SOURCE to TARGET through a same-directory temporary file.  The final
  # rename is atomic, so readers observe either the old complete file or the
  # new complete file, never a partially copied JAR.  Re-check the source after
  # copying as a defensive measure for a source cache being populated at the
  # same time; an unstable source is not materialized.
  local source=$1 target=$2 source_size source_hash source_size_after source_hash_after
  local temp temp_size temp_hash attempt
  [[ -s $source && -f $source ]] || {
    print -u2 "toolchain source is missing or empty: $source"
    return 1
  }
  mkdir -p "${target:h}" || return 1
  for attempt in {1..3}; do
    source_size=$(fixture_file_size "$source") || return 1
    source_hash=$(fixture_file_hash "$source") || return 1
    if fixture_materialized_file_valid "$source" "$target" "$source_size" "$source_hash"; then
      return 0
    fi
    temp=$(mktemp "${target}.tmp.XXXXXX") || return 1
    if ! cp "$source" "$temp"; then
      rm -f "$temp"
      return 1
    fi
    temp_size=$(fixture_file_size "$temp" 2>/dev/null || true)
    temp_hash=$(fixture_file_hash "$temp" 2>/dev/null || true)
    source_size_after=$(fixture_file_size "$source" 2>/dev/null || true)
    source_hash_after=$(fixture_file_hash "$source" 2>/dev/null || true)
    if [[ $temp_size != $source_size || $temp_hash != $source_hash ||
          $source_size_after != $source_size || $source_hash_after != $source_hash ]]; then
      rm -f "$temp"
      continue
    fi
    if ! mv -f "$temp" "$target"; then
      rm -f "$temp"
      return 1
    fi
    fixture_materialized_file_valid "$source" "$target" "$source_size" "$source_hash" && return 0
  done
  print -u2 "toolchain source changed while materializing: $source"
  return 1
}

fixture_argument_vector_digest() {
  # Hash the argument count and NUL-delimited values. Shell arguments cannot
  # contain NUL, so this preserves empty arguments and every boundary without
  # the collisions caused by flattening "$*".
  {
    printf '%d\0' "$#"
    for argument in "$@"; do
      printf '%s\0' "$argument"
    done
  } | shasum -a 256 | awk '{print $1}'
}

fixture_source_digest() {
  # Usage: fixture_source_digest BASE FILE ...
  # Include relative names as well as bytes: adding/removing/renaming a source
  # cannot accidentally reuse a cache produced for the old source set.
  local base=$1
  shift
  [[ -r $FIXTURE_DIGEST_HELPER ]] || {
    print -u2 "fixture digest helper is missing: $FIXTURE_DIGEST_HELPER"; return 1
  }
  LC_ALL=C perl "$FIXTURE_DIGEST_HELPER" source "$base" "$@"
}

fixture_generated_content_digest() {
  # Digest generated source contents without including the marker itself.
  # BASE is the common stable root; one or more generated output directories
  # may follow (Cats has two roots).
  local base=$1
  shift
  [[ -r $FIXTURE_DIGEST_HELPER ]] || {
    print -u2 "fixture digest helper is missing: $FIXTURE_DIGEST_HELPER"; return 1
  }
  LC_ALL=C perl "$FIXTURE_DIGEST_HELPER" generated "$base" "$@"
}

fixture_toolchain_path() {
  local key=$1 toolchain_path env_value
  case $key in
    scala_home) env_value=${SCALA_HOME:-} ;;
    scala_library) env_value=${SCALA_LIBRARY_JAR:-${SCALA_LIBRARY:-}} ;;
    scala_compiler_jar) env_value=${SCALA_COMPILER_JAR:-} ;;
    scala_library_jar) env_value=${SCALA_LIBRARY_JAR:-${SCALA_LIBRARY:-}} ;;
    scala_launcher_library_jar) env_value=${SCALA_LAUNCHER_LIBRARY_JAR:-} ;;
    scala_reflect_jar) env_value=${SCALA_REFLECT_JAR:-} ;;
    scalac_launcher) env_value=${SCALAC:-} ;;
  esac
  # scala_library_jar was the original key used by the cache code.  Keep the
  # two library spellings as aliases in either direction so private manifests
  # may describe the library once while the public manifest remains explicit.
  if [[ -n $env_value ]]; then
    toolchain_path=$env_value
  else
    toolchain_path=$(fixture_cfg toolchain "$key" 2>/dev/null || true)
    if [[ -z $toolchain_path && $key == scala_library ]]; then
      toolchain_path=$(fixture_cfg toolchain scala_library_jar 2>/dev/null || true)
    elif [[ -z $toolchain_path && $key == scala_library_jar ]]; then
      toolchain_path=$(fixture_cfg toolchain scala_library 2>/dev/null || true)
    fi
  fi
  [[ -n $toolchain_path ]] || return 1
  # Toolchain paths are deliberately separate from fixture/cache paths.  The
  # default remains /tmp (as it was before the manifest existed), while a
  # caller can put the complete toolchain elsewhere with one root override.
  [[ $toolchain_path = /* ]] || toolchain_path=${SCALA_RS_TOOLCHAIN_ROOT:-/tmp}/$toolchain_path
  print -r -- "$toolchain_path"
}

fixture_toolchain_home() {
  fixture_toolchain_path scala_home
}

fixture_toolchain_version() {
  local version
  version=$(fixture_cfg toolchain scala_version 2>/dev/null || true)
  print -r -- "${version:-2.13.16}"
}

fixture_toolchain_library() {
  fixture_toolchain_path scala_library
}

fixture_toolchain_launcher_library() {
  fixture_toolchain_path scala_launcher_library_jar
}

fixture_toolchain_compiler() {
  fixture_toolchain_path scala_compiler_jar
}

fixture_toolchain_reflect() {
  fixture_toolchain_path scala_reflect_jar
}

fixture_toolchain_digest() {
  local key toolchain_path hash
  {
    for key in scala_compiler_jar scala_library_jar scala_launcher_library_jar scala_reflect_jar; do
      toolchain_path=$(fixture_toolchain_path "$key" 2>/dev/null || true)
      hash=$(fixture_file_hash "$toolchain_path" 2>/dev/null || true)
      [[ -n $hash ]] || hash=missing
      print -r -- "$key=${toolchain_path:-missing} hash=$hash"
    done
  } | LC_ALL=C sort | shasum -a 256 | awk '{print $1}'
}

fixture_scalac_path() {
  fixture_toolchain_path scalac_launcher
}

fixture_java_path() {
  local java_path=${JAVA:-}
  if [[ -n $java_path ]]; then
    [[ $java_path = */* ]] || java_path=$(command -v "$java_path" 2>/dev/null || true)
  elif [[ -n ${JAVA_HOME:-} && -x ${JAVA_HOME}/bin/java ]]; then
    java_path=${JAVA_HOME}/bin/java
  else
    java_path=$(command -v java 2>/dev/null || true)
  fi
  [[ -n $java_path ]] || {
    print -u2 'java executable is not available (set JAVA or JAVA_HOME)'
    return 1
  }
  [[ -x $java_path ]] || {
    print -u2 "java executable is not runnable: $java_path"
    return 1
  }
  print -r -- "$java_path"
}

fixture_javac_path() {
  local javac_path=${JAVAC:-}
  if [[ -n $javac_path ]]; then
    [[ $javac_path = */* ]] || javac_path=$(command -v "$javac_path" 2>/dev/null || true)
  elif [[ -n ${JAVA_HOME:-} && -x ${JAVA_HOME}/bin/javac ]]; then
    javac_path=${JAVA_HOME}/bin/javac
  else
    javac_path=$(command -v javac 2>/dev/null || true)
  fi
  [[ -n $javac_path ]] || {
    print -u2 'javac executable is not available (set JAVAC or JAVA_HOME)'
    return 1
  }
  [[ -x $javac_path ]] || {
    print -u2 "javac executable is not runnable: $javac_path"
    return 1
  }
  print -r -- "$javac_path"
}

fixture_write_scalac_launcher() {
  # Generate the tiny nsc launcher from the manifest-resolved paths.  An
  # explicitly supplied SCALAC is authoritative and must never be replaced.
  [[ -z ${SCALAC:-} ]] || return 0
  local launcher compiler library reflect java_bin tmp
  launcher=$(fixture_scalac_path) || return 1
  compiler=$(fixture_toolchain_compiler) || return 1
  library=$(fixture_toolchain_launcher_library) || return 1
  reflect=$(fixture_toolchain_reflect) || return 1
  java_bin=$(fixture_java_path) || return 1
  mkdir -p "${launcher:h}"
  tmp=$(mktemp "${launcher}.XXXXXX") || return 1
  {
    print -r -- '#!/bin/sh'
    printf 'JAVA=%q\n' "$java_bin"
    printf 'CP=%q:%q:%q\n' "$compiler" "$library" "$reflect"
    print -r -- 'exec "$JAVA" -Dscala.usejavacp=true -cp "$CP" scala.tools.nsc.Main "$@"'
  } > "$tmp" || { rm -f "$tmp"; return 1; }
  chmod +x "$tmp" && mv -f "$tmp" "$launcher"
}

fixture_prepare_toolchain() {
  # Usage: fixture_prepare_toolchain COURSIER_CACHE
  # Materialise only missing jars, then always refresh the generated launcher
  # atomically.  Existing /tmp defaults and explicit path overrides remain
  # valid; this merely makes their locations come from the manifest/helper.
  local ccache=${1:-} src target library compiler launcher_library reflect version
  local -a targets sources
  [[ -n $ccache ]] || ccache=${HOME}/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
  version=$(fixture_toolchain_version)
  library=$(fixture_toolchain_library) || return 1
  launcher_library=$(fixture_toolchain_launcher_library) || return 1
  compiler=$(fixture_toolchain_compiler) || return 1
  reflect=$(fixture_toolchain_reflect) || return 1
  targets=("$library" "$launcher_library" "$compiler" "$reflect")
  sources=(
    "$ccache/org/scala-lang/scala-library/$version/scala-library-$version.jar"
    "$ccache/org/scala-lang/scala-library/$version/scala-library-$version.jar"
    "$ccache/org/scala-lang/scala-compiler/$version/scala-compiler-$version.jar"
    "$ccache/org/scala-lang/scala-reflect/$version/scala-reflect-$version.jar"
  )
  for i in {1..4}; do
    target=${targets[$i]}
    src=${sources[$i]}
    fixture_materialize_file "$src" "$target" || {
      print -u2 "could not materialize Scala toolchain jar: $target"
      return 1
    }
  done
  fixture_write_scalac_launcher
}

fixture_output_digest() {
  local out=$1
  [[ -r $FIXTURE_DIGEST_HELPER ]] || {
    print -u2 "fixture digest helper is missing: $FIXTURE_DIGEST_HELPER"; return 1
  }
  LC_ALL=C perl "$FIXTURE_DIGEST_HELPER" output "$out"
}

fixture_classpath_digest() {
  # Include the ordered classpath and bytes of every existing entry.  Staging
  # directories (notably gitbucket's javac output) are represented by their
  # content digest, not their temporary pathname, so measure and run derive
  # the same key.
  local cp=$1
  local -a entries
  entries=("${(@s.:.)cp}")
  [[ -r $FIXTURE_DIGEST_HELPER ]] || {
    print -u2 "fixture digest helper is missing: $FIXTURE_DIGEST_HELPER"; return 1
  }
  LC_ALL=C perl "$FIXTURE_DIGEST_HELPER" classpath "${entries[@]}"
}

fixture_validate_classpath() {
  local cp=$1 entry
  for entry in ${(s.:.)cp}; do
    [[ -f $entry || -d $entry ]] || {
      print -u2 "fixture classpath entry is missing: $entry"; return 1
    }
  done
}

fixture_dependency_digest() {
  # Digest both the dependency-list file's paths and the bytes behind them.
  # This makes a completed marker stale when a resolved jar disappears or is
  # replaced, not only when the text file itself changes.
  local file=$1 cp
  [[ -s $file ]] || return 1
  cp=$(cat "$file")
  fixture_validate_classpath "$cp" || return 1
  fixture_classpath_digest "$cp"
}

fixture_jdk_fingerprint() {
  local java_bin
  java_bin=$(fixture_java_path) || return 1
  # Include the selected executable as well as its version.  Two JDKs can
  # report the same version while differing in vendor/runtime behavior, and
  # this also guarantees the key records the binary actually used by runs.
  print -r -- "path=$java_bin version=$("$java_bin" -version 2>&1 | head -1)"
}

fixture_artifact_key() {
  # fixture_artifact_key FIXTURE KIND COMPILER REV SOURCE_DIGEST FLAGS CP
  local fixture=$1 kind=$2 compiler=$3 rev=$4 source_digest=$5 flags=$6 cp=$7
  local compiler_hash scalac_hash scalac_path
  compiler_hash=$(fixture_file_hash "$compiler") || compiler_hash=missing
  scalac_path=$(fixture_scalac_path)
  scalac_hash=$(fixture_file_hash "$scalac_path" 2>/dev/null || true)
  [[ -n $scalac_hash ]] || scalac_hash=missing
  {
    print -r -- "fixture=$fixture"
    print -r -- "kind=$kind"
    print -r -- "compiler_hash=$compiler_hash"
    print -r -- "upstream_revision=$rev"
    print -r -- "source_digest=$source_digest"
    print -r -- "flags=$flags"
    print -r -- "classpath=$(fixture_classpath_digest "$cp")"
    print -r -- "scala_toolchain=$(fixture_toolchain_digest)"
    print -r -- "jdk=$(fixture_jdk_fingerprint)"
    print -r -- "scalac_path=$scalac_path"
    print -r -- "scalac_hash=$scalac_hash"
  } | shasum -a 256 | awk '{print $1}'
}

fixture_manifest_file() {
  local dir=$1 key=$2
  [[ -s $dir/manifest ]] || return 1
  [[ $(grep -Fxc "artifact_key=$key" "$dir/manifest") == 1 ]]
}

fixture_artifact_valid() {
  # Usage: fixture_artifact_valid DIR KEY SOURCE_COUNT FIXTURE KIND COMPILER
  #        REV SOURCE_DIGEST FLAGS CP
  # The expanded arguments let validation recompute the complete fingerprint;
  # a matching opaque key alone cannot prove that a hand-edited manifest is
  # consistent with the current compiler, classpath, or JDK.
  local dir=$1 key=$2 expected_count=$3 fixture=$4 kind=$5 compiler=$6
  local rev=$7 source_digest=$8 flags=$9 cp=${10} count field
  local expected_scalac expected_scalac_hash expected_compiler_hash expected_cp_digest expected_toolchain expected_jdk expected_key
  [[ -d $dir/out && -f $dir/COMPLETE ]] || return 1
  fixture_manifest_file "$dir" "$key" || return 1
  [[ $expected_count == <-> ]] || return 1
  # A key alone is not enough: a hand-created or interrupted manifest must not
  # be mistaken for a build fingerprint.  The fields are deliberately plain
  # text so humans can inspect a cache entry after a failure.
  for field in fixture kind compiler_hash upstream_revision source_digest flags classpath_digest scala_toolchain jdk scalac scalac_hash source_count output_digest; do
    [[ $(grep -Ec "^$field=" "$dir/manifest") == 1 ]] || return 1
  done
  [[ $(grep -Fxc "fixture=$fixture" "$dir/manifest") == 1 ]] || return 1
  [[ $(grep -Fxc "kind=$kind" "$dir/manifest") == 1 ]] || return 1
  [[ $(grep -Fxc "source_count=$expected_count" "$dir/manifest") == 1 ]] || return 1
  [[ $(grep -Fxc "upstream_revision=$rev" "$dir/manifest") == 1 ]] || return 1
  [[ $(grep -Fxc "source_digest=$source_digest" "$dir/manifest") == 1 ]] || return 1
  [[ $(grep -Fxc "flags=$flags" "$dir/manifest") == 1 ]] || return 1
  expected_cp_digest=$(fixture_classpath_digest "$cp")
  expected_toolchain=$(fixture_toolchain_digest)
  expected_jdk=$(fixture_jdk_fingerprint) || return 1
  expected_compiler_hash=$(fixture_file_hash "$compiler" 2>/dev/null || print missing)
  [[ $(grep -Fxc "classpath_digest=$expected_cp_digest" "$dir/manifest") == 1 ]] || return 1
  [[ $(grep -Fxc "scala_toolchain=$expected_toolchain" "$dir/manifest") == 1 ]] || return 1
  [[ $(grep -Fxc "jdk=$expected_jdk" "$dir/manifest") == 1 ]] || return 1
  [[ $(grep -Fxc "compiler_hash=$expected_compiler_hash" "$dir/manifest") == 1 ]] || return 1
  expected_scalac=$(fixture_scalac_path)
  expected_scalac_hash=$(fixture_file_hash "$expected_scalac" 2>/dev/null || print missing)
  [[ $(grep -Fxc "scalac=$expected_scalac" "$dir/manifest") == 1 ]] || return 1
  [[ $(grep -Fxc "scalac_hash=$expected_scalac_hash" "$dir/manifest") == 1 ]] || return 1
  expected_key=$(fixture_artifact_key "$fixture" "$kind" "$compiler" "$rev" "$source_digest" "$flags" "$cp")
  [[ $expected_key == $key ]] || return 1
  count=$(find "$dir/out" -type f -name '*.class' | wc -l | tr -d ' ')
  (( count > 0 )) || return 1
  [[ $(grep -Fxc "output_digest=$(fixture_output_digest "$dir/out")" "$dir/manifest") == 1 ]] || return 1
}

fixture_artifact_destination_valid() {
  # Usage: fixture_artifact_destination_valid DIR KEY STAGE_MANIFEST
  # Validate an existing complete destination against the new stage's key and
  # metadata, then verify the destination's recorded digest against its own
  # output.  The stage's output may differ (for example from compiler output
  # ordering), so its digest is not used to decide whether the first complete
  # destination wins.
  local dir=$1 key=$2 stage_manifest=$3 field expected count
  [[ -d $dir && -f $dir/COMPLETE ]] || return 1
  fixture_manifest_file "$dir" "$key" || return 1
  for field in fixture kind source_count compiler_hash upstream_revision source_digest flags classpath_digest scala_toolchain jdk scalac scalac_hash; do
    expected=$(grep -m1 -E "^$field=" "$stage_manifest") || return 1
    [[ $(grep -Fxc "$expected" "$dir/manifest") == 1 ]] || return 1
  done
  [[ -d $dir/out ]] || return 1
  count=$(find "$dir/out" -type f -name '*.class' | wc -l | tr -d ' ')
  (( count > 0 )) || return 1
  expected=$(fixture_output_digest "$dir/out") || return 1
  [[ $(grep -Fxc "output_digest=$expected" "$dir/manifest") == 1 ]]
}

fixture_artifact_stage() {
  local fixture=$1 kind=$2 key=$3 parent
  parent=$FIXTURE_CACHE/$fixture/$kind
  mkdir -p "$parent"
  mktemp -d "$parent/.stage-$key.XXXXXX"
}

fixture_artifact_publish() {
  # Usage: fixture_artifact_publish STAGE DEST KEY METADATA...
  # Metadata arguments are key=value lines.  COMPLETE is created last, and the
  # directory is then renamed into place. Readers therefore never accept a
  # partially-written tree, even if compilation is interrupted.
  local stage=$1 dest=$2 key=$3 item output_digest publish_lock wait_count publish_status
  shift 3
  for item in "$@"; do print -r -- "$item" >> "$stage/manifest"; done
  print -r -- "artifact_key=$key" >> "$stage/manifest"
  for item in fixture kind source_count compiler_hash upstream_revision source_digest flags classpath_digest scala_toolchain jdk scalac scalac_hash; do
    [[ $(grep -Ec "^$item=.+" "$stage/manifest") == 1 ]] || {
      print -u2 "refusing to publish artifact without exact $item metadata: $stage"; return 1
    }
  done
  [[ -d $stage/out ]] || { print -u2 "refusing to publish artifact without out/: $stage"; return 1; }
  [[ $(find "$stage/out" -type f -name '*.class' | wc -l | tr -d ' ') -gt 0 ]] || {
    print -u2 "refusing to publish artifact without classfiles: $stage"; return 1
  }
  output_digest=$(fixture_output_digest "$stage/out") || return 1
  print -r -- "output_digest=$output_digest" >> "$stage/manifest"
  [[ $(grep -Fxc "artifact_key=$key" "$stage/manifest") == 1 ]] || {
    print -u2 "refusing to publish artifact with an inexact key: $stage"; return 1
  }
  [[ $(grep -Ec '^output_digest=.+' "$stage/manifest") == 1 ]] || {
    print -u2 "refusing to publish artifact with an inexact output digest: $stage"; return 1
  }
  : > "$stage/COMPLETE"
  # Serialise the final check-and-rename with an atomic mkdir lock.  Two
  # publishers for the same key may finish their private stages together;
  # whichever acquires this lock first publishes its complete tree, while the
  # other waits and then discards its stage after observing that tree.  The
  # lock helper also reclaims a dead owner's lock, so an interrupted publisher
  # cannot strand later writers indefinitely.
  publish_lock=$dest.publish.lock
  wait_count=0
  while :; do
    if fixture_lock_acquire "$publish_lock" "$FIXTURE_CACHE" 2>/dev/null; then
      break
    fi
    wait_count=$((wait_count + 1))
    if (( wait_count >= 3000 )); then
      print -u2 "timed out waiting to publish artifact: $dest"
      fixture_safe_clean "$FIXTURE_CACHE" "$stage"
      return 1
    fi
    sleep 0.01
  done

  # Validate the destination only while holding the publish lock.  Preserve a
  # complete artifact only when its key/metadata and its own output digest are
  # intact; otherwise repair corrupted cache debris with this valid stage.
  if fixture_artifact_destination_valid "$dest" "$key" "$stage/manifest"; then
    fixture_safe_clean "$FIXTURE_CACHE" "$stage"
    publish_status=$?
  else
    if [[ -e $dest ]]; then
      fixture_safe_clean "$FIXTURE_CACHE" "$dest" || {
        fixture_lock_release "$publish_lock" "$FIXTURE_CACHE"
        fixture_safe_clean "$FIXTURE_CACHE" "$stage"
        return 1
      }
    fi
    mv "$stage" "$dest"
    publish_status=$?
  fi
  fixture_lock_release "$publish_lock" "$FIXTURE_CACHE"
  return $publish_status
}

fixture_generated_mark() {
  # Usage: fixture_generated_mark FIXTURE DIR REV INPUT_DIGEST COUNT STATE DIGEST
  local fixture=$1 dir=$2 rev=$3 digest=$4 count=$5 state=$6 generated_digest=$7 tmp
  tmp=$(mktemp "$dir/.fixture-generated.XXXXXX") || return 1
  {
    print -r -- "fixture=$fixture"
    print -r -- "upstream_revision=$rev"
    print -r -- "input_digest=$digest"
    print -r -- "generated_count=$count"
    print -r -- "generated_state=$state"
    print -r -- "generated_digest=$generated_digest"
  } > "$tmp" || { rm -f "$tmp"; return 1; }
  mv -f "$tmp" "$dir/.fixture-generated"
}

fixture_generated_valid() {
  # Usage: fixture_generated_valid FIXTURE DIR REV INPUT_DIGEST COUNT STATE DIGEST
  # The caller validates generated_count over its selected output roots (cats
  # has two roots); this predicate validates only the atomic state record.
  local fixture=$1 dir=$2 rev=$3 digest=$4 count=$5 state=$6 generated_digest=$7
  [[ -s $dir/.fixture-generated ]] || return 1
  grep -Fqx "fixture=$fixture" "$dir/.fixture-generated" || return 1
  grep -Fqx "upstream_revision=$rev" "$dir/.fixture-generated" || return 1
  grep -Fqx "input_digest=$digest" "$dir/.fixture-generated" || return 1
  grep -Fqx "generated_count=$count" "$dir/.fixture-generated" || return 1
  grep -Fqx "generated_state=$state" "$dir/.fixture-generated" || return 1
  grep -Fqx "generated_digest=$generated_digest" "$dir/.fixture-generated" || return 1
}

fixture_state_mark() {
  # Generic completion marker for derived dependency/classpath trees.
  # Usage: fixture_state_mark MARKER FIXTURE REV INPUT_DIGEST CONTENT_DIGEST STATE
  local marker=$1 fixture=$2 rev=$3 input_digest=$4 content_digest=$5 state=$6 tmp
  mkdir -p "${marker:h}"
  tmp=$(mktemp "${marker}.XXXXXX") || return 1
  {
    print -r -- "fixture=$fixture"
    print -r -- "upstream_revision=$rev"
    print -r -- "input_digest=$input_digest"
    print -r -- "content_digest=$content_digest"
    print -r -- "state=$state"
  } > "$tmp" || { rm -f "$tmp"; return 1; }
  mv -f "$tmp" "$marker"
}

fixture_state_valid() {
  # Usage: fixture_state_valid MARKER FIXTURE REV INPUT_DIGEST CONTENT_DIGEST STATE
  local marker=$1 fixture=$2 rev=$3 input_digest=$4 content_digest=$5 state=$6
  [[ -s $marker ]] || return 1
  grep -Fqx "fixture=$fixture" "$marker" || return 1
  grep -Fqx "upstream_revision=$rev" "$marker" || return 1
  grep -Fqx "input_digest=$input_digest" "$marker" || return 1
  grep -Fqx "content_digest=$content_digest" "$marker" || return 1
  grep -Fqx "state=$state" "$marker" || return 1
}

fixture_validate_source_count() {
  local fixture=$1 actual=$2 variant=${3:-default} expected key
  key=source_count
  [[ $variant == default ]] || key="source_count_$variant"
  expected=$(fixture_cfg "$fixture" "$key")
  [[ -n $expected && $actual == $expected ]] || {
    print -u2 "measurement invalid: $fixture source count $actual, expected ${expected:-unknown} ($variant)"
    return 1
  }
}

fixture_validate_generated() {
  # Usage: fixture_validate_generated FIXTURE GENERATED_SOURCE_COUNT STATE
  local fixture=$1 actual=$2 state=$3 expected expected_state
  expected=$(fixture_cfg "$fixture" generated_count)
  expected_state=$(fixture_cfg "$fixture" generated_state)
  [[ -n $expected && $actual == $expected ]] || {
    print -u2 "measurement invalid: $fixture generated source count $actual, expected ${expected:-unknown}"; return 1
  }
  [[ $state == $expected_state ]] || {
    print -u2 "measurement invalid: $fixture generated state '$state', expected '$expected_state'"; return 1
  }
}
