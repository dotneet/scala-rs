// SLS 5.1: `class X extends Loud` where `trait Loud extends Animal` makes
// `Animal` X's superclass -- on the JVM too, or `val a: Animal = new X` cannot
// be verified.
object Main {
  abstract class Animal {
    def speak: String = "base"
    def hi: String = "hi"
  }
  trait Loud extends Animal { abstract override def speak = "LOUD-" + super.speak }
  trait Bang extends Animal { abstract override def speak = super.speak + "!" }
  class X extends Loud
  class Y extends Loud with Bang

  def main(args: Array[String]): Unit = {
    val x = new X
    println(x.speak)
    println(x.hi)
    val a: Animal = x
    println(a.speak)
    val y: Animal = new Y
    println(y.speak)
  }
}
