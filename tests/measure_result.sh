# Shared by the zsh compile-measure scripts. A normal diagnostic exit is a
# useful measurement; a crash or an empty source/output set is not.
validate_measure_result() {
  local compiler_exit=$1 errors=$2 classes=$3 files=$4 log=$5
  if [ "$files" -eq 0 ]; then
    echo "measurement invalid: no source files (log: $log)" >&2
    return 2
  fi
  if [ "$compiler_exit" -eq 1 ] && [ "$errors" -gt 0 ]; then
    return 0
  fi
  if [ "$compiler_exit" -ne 0 ] || [ "$errors" -ne 0 ] || [ "$classes" -eq 0 ]; then
    echo "measurement invalid: compiler_exit=$compiler_exit errors=$errors classes=$classes (log: $log)" >&2
    return 2
  fi
}
