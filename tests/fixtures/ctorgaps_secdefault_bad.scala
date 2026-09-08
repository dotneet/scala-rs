// A default getter is named after the parameter *position* and nothing else,
// so two constructors that both define defaults want the same
// `$lessinit$greater$default$2` with different bodies. nsc forbids that for
// any overloaded method, constructors included, which is why the position is
// enough of a name for it too.
//
// scalac 2.13.16 reports, at line 9:
//   in class Two, multiple overloaded alternatives of constructor Two define
//   default arguments.
//
// Its caret is under `Two` and ours under `class`; the message is its own.
// Kept in its own file because nsc reports this in a later phase than the
// `not found` of `ctorgaps_secthis_bad.scala`, so putting both in one file
// silences whichever comes second.
class Two(val v: String) {
  def this(n: Int, sep: String = "-") = this(sep + n)
  def this(f: Boolean, tag: String = "t") = this(tag + f)
}
