// A trait whose superclass is `Animal` may only be mixed into a subclass of
// `Animal`. scalac 2.13.16: `illegal inheritance; superclass Plain / is not a
// subclass of the superclass Animal / of the mixin trait Loud`, once at the
// named class and once at the anonymous one.
object Main {
  abstract class Animal(val name: String) { def speak: String }
  trait Loud extends Animal { abstract override def speak = "LOUD-" + super.speak }
  class Plain { def speak: String = "x" }
  class Y extends Plain with Loud
  def main(args: Array[String]): Unit = {
    println(new Y().speak)
    println(new Plain with Loud)
  }
}
