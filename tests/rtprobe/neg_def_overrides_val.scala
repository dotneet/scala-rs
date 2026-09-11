// scalac: stable, immutable value required to override (a def cannot
// override a val).
object Main {
  class A { val x: Int = 1 }
  class B extends A { override def x: Int = 2 }
  def main(args: Array[String]): Unit = println(new B().x)
}
