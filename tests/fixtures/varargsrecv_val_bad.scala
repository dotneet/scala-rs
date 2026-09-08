// `T*` is a parameter's declaration form and not a type: a `val` may not have
// one. scalac 2.13.16 refuses it in the parser (`';' expected but identifier
// found.`); this compiler parses the `*` and refuses the definition. Both
// reject; the wording is not nsc's, and that is a parser difference this slice
// did not introduce and does not close.
object Main {
  val v: Int* = null
}
