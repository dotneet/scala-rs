// A trait never runs its superclass's constructor, so it may not write an
// argument list. scalac 2.13.16: `parents of traits may not have parameters`.
object Main {
  abstract class Animal(val name: String) { def speak: String }
  trait Loud extends Animal("x") { abstract override def speak = "LOUD-" + super.speak }
  def main(args: Array[String]): Unit = println("unreachable")
}
