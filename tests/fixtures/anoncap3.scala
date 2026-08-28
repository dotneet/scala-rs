// Captured `var`s and local classes.
trait Runner { def run(): Unit }

object Main {
  // Writes from inside the anonymous class are visible to the method.
  def counter(): Int = {
    var n = 0
    val r = new Runner {
      def run(): Unit = { n = n + 1 }
    }
    r.run()
    r.run()
    r.run()
    n
  }

  // A local class with its own constructor parameter plus a capture.
  def localClass(y: Int): Int = {
    class Inner(val extra: Int) {
      def get(): Int = y * 2 + extra
    }
    new Inner(1).get()
  }

  // A `var` and a `val` captured by the same anonymous class.
  def captureBoth(seed: Int): String = {
    var acc = seed
    val label = "acc"
    val r = new Runner {
      def run(): Unit = { acc = acc * 2 }
    }
    r.run()
    r.run()
    label + "=" + acc
  }

  // The same instance keeps writing the captured `var` across loop turns.
  def counterLoop(times: Int): Int = {
    var total = 0
    var i = 0
    val r = new Runner {
      def run(): Unit = { total = total + i }
    }
    while (i < times) {
      r.run()
      i = i + 1
    }
    total
  }

  // A by-name parameter captured by an anonymous class.
  def byName(t: => Int): Int = {
    val r = new Runner {
      def run(): Unit = println("byName " + t)
    }
    r.run()
    t
  }

  def main(args: Array[String]): Unit = {
    println(counter())
    println(localClass(3))
    println(captureBoth(5))
    println(counterLoop(4))
    println(byName(6))
  }
}
