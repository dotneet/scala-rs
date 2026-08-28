// Captures combined with `$outer`, lambdas and nesting.
trait Runner { def run(): Unit }
trait Adder { def add(k: Int): Int }

class Holder(val base: Int) {
  // Reads both a member of the enclosing class and a captured parameter.
  def mk(x: Int): Runner = new Runner {
    def run(): Unit = println("holder " + (base + x))
  }
}

trait Maker {
  def base: Int
  // The enclosing instance here is the trait: `$outer` is typed as the
  // interface and the receiver comes from the `$class` static implementation.
  def make(x: Int): Adder = new Adder {
    def add(k: Int): Int = k + base + x
  }
}

class Impl extends Maker { def base: Int = 100 }
object ImplObj extends Maker { def base: Int = 200 }

object Main {
  // A lambda inside the anonymous class captures the same local again.
  def dbl(m: Int): Adder = new Adder {
    def add(k: Int): Int = {
      val f = (i: Int) => i * m + k
      f(2)
    }
  }

  // The inner anonymous class reaches a local of the outermost method.
  def nested(p: Int): Runner = new Runner {
    def run(): Unit = {
      val inner = new Runner {
        def run(): Unit = println("inner " + p)
      }
      inner.run()
    }
  }

  // The anonymous class is built inside a lambda and captures both the
  // lambda's parameter and the method's.
  def inLambda(n: Int): Int = {
    val f = (m: Int) => {
      val r = new Adder { def add(k: Int): Int = k + n + m }
      r.add(1)
    }
    f(10)
  }

  // The anonymous class lives in a nested `def`, which lambda-lift turns into
  // a synthetic method that must forward the capture.
  def inNestedDef(n: Int): Int = {
    def make(): Adder = new Adder { def add(k: Int): Int = k * n }
    make().add(3)
  }

  def main(args: Array[String]): Unit = {
    new Holder(10).mk(5).run()
    println(dbl(5).add(4))
    nested(42).run()
    println(inLambda(5))
    println(inNestedDef(4))
    println(new Impl().make(5).add(1))
    println(ImplObj.make(5).add(1))
  }
}
