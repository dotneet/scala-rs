// The nested-`object` shape that must be diagnosed rather than miscompiled.
//
// `class VC(val u: Int) extends AnyVal { object Inner }` used to be here too.
// It is a *rejection rule* -- nsc's "implementation restriction: nested
// object is not allowed in value class" -- and the pass that reports the
// shape below runs only when nothing else has errored, so the two cannot
// share a file: the value-class error suppressed this one. The value-class
// restriction and its neighbours (nested class, nested trait, and the same
// rules inside a `def` body) are pinned in
// `tests/fixtures/negchecks_ephemeral_bad.scala`.
object Main {
  class Outer(val v: Int) {
    // A local `object` that reads the enclosing instance is not compiled yet
    // (nsc holds it in a per-call `scala.runtime.LazyRef`); saying so beats
    // emitting a static singleton that dies with `NoSuchFieldError: $outer`.
    def m(k: Int): Int = {
      object L { def g = v + k }
      L.g
    }
  }
  def main(args: Array[String]): Unit = println(new Outer(1).m(2))
}
