// Scanner errors scala-rs used to let through: a `$` in an interpolation
// that starts no hole, and `\$` in a literal (not an escape).
object Test {
  val x = 1
  def interp = s"a$ b"
  def escaped = "a\$b"
}
