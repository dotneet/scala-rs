// Lambda-lift hoists every nested `def` onto the enclosing class, so a def
// that *calls* another one has to be able to hand it its captures — by the
// time the caller's own captures are computed, the callee has already been
// pulled out of its body and nothing there mentions them any more. Three
// levels deep, and through a `lazy val` accessor, which is a hoisted def too.
object Main {
  def chain(n: Int): Int = {
    def inner(m: Int): Int = {
      def g: Int = m + n
      g + g
    }
    inner(1) + inner(2)
  }

  def deeper(n: Int): Int = {
    def a(x: Int): Int = {
      def b(y: Int): Int = {
        def c: Int = x + y + n
        c
      }
      b(10) + b(20)
    }
    a(1)
  }

  // The accessor for `v` captures `m` and `n`; `inner` reads neither by
  // itself, so both have to reach it as captures too.
  def viaLazy(n: Int): Int = {
    def inner(m: Int): Int = {
      lazy val v: Int = { println("v " + m); m * 100 + n }
      v + v
    }
    inner(1) + inner(2)
  }

  def main(args: Array[String]): Unit = {
    println(chain(10))
    println(deeper(100))
    println(viaLazy(7))
  }
}
