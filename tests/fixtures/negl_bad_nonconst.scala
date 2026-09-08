// The fold added by this slice is deliberately narrow: it only recovers the
// constant-ness of `-<literal>`. `-n` for a non-constant `n` is not a
// constant expression under SLS 6.24, so it must not narrow, and the
// rejection must be the plain `Int`/`Byte` mismatch scalac gives -- not
// something that looks like the fold half-applied.
object NeglBadNonconst {
  val n = 3
  val b: Byte = -n
}
