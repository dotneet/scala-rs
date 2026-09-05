// Two things that compiled clean and then failed to link.
//
//  1. A class or object declared *inside a local class* never reached a
//     classfile. The walk that emits local templates stopped at the local
//     class itself, so `def f = { class Outer { class Inner } }` wrote
//     `Main$Outer$1` and nothing else, and the first `new Inner` threw
//     `NoClassDefFoundError: Main$Outer$1$Inner`.
//
//  2. `import Helper.member` where `member` is *inherited* by the object from
//     a superclass: the name enters the scope, but the symbol's owner is the
//     class, so the backend had no receiver to load and fell back to `this`
//     (`ClassCastException: class Main$ cannot be cast to class Base`).
class Base(val seed: Int) {
  def scaled(n: Int): Int = n * 2 + seed
  val label: String = "base"
}

object Helper extends Base(1)

import Helper.scaled
import Helper.label

object Main {
  def localNesting(): String = {
    class Outer {
      class Inner {
        def describe: String = "inner"
      }
      object Companionless {
        def describe: String = "object"
      }
      def make: String = new Inner().describe + "/" + Companionless.describe
    }
    new Outer().make
  }

  def main(args: Array[String]): Unit = {
    println(localNesting())
    println(scaled(20))
    println(label)
  }
}
