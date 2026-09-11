// Early definitions (deprecated in 2.13 but legal) run before the parent
// trait's initializer; compare with a plain subclass val.
object Main {
  trait Greeter { val name: String; val msg = "Hello, " + name }
  class Late extends Greeter { val name = "late" }
  class Early extends { val name = "early" } with Greeter
  object EarlyObj extends { val name = "object" } with Greeter
  abstract class AbsC { val n: Int; val sq = n * n }
  class EarlyC extends { val n = 7 } with AbsC

  def main(args: Array[String]): Unit = {
    println(new Late().msg)
    println(new Early().msg)
    println(EarlyObj.msg)
    println(new EarlyC().sq)
  }
}
