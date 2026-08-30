// Two nested-`object` shapes that must be diagnosed rather than miscompiled.
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
  // scalac rejects this outright, in these words.
  class VC(val u: Int) extends AnyVal {
    object Inner { def f = u + 1 }
  }
  def main(args: Array[String]): Unit = println(new Outer(1).m(2))
}
