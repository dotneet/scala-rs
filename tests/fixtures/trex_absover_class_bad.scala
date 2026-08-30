// `abstract override` needs a linearized `super`, which only a trait has.
// scalac 2.13.16: "`abstract override` modifier only allowed for members of
// traits".
object Main {
  abstract class Animal { def speak: String }
  class Cls extends Animal { abstract override def speak = "LOUD-" + super.speak }
  def main(args: Array[String]): Unit = println("unreachable")
}
