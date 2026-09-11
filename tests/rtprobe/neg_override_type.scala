// scalac: incompatible type in overriding def f: Int; method f has
// incompatible type.
object Main {
  class A { def f: Int = 1 }
  class B extends A { override def f: String = "s" }
  def main(args: Array[String]): Unit = println(new B().f)
}
