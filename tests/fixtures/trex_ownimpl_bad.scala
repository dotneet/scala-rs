// The class's own `speak` sits *above* `Loud` in the linearization, so it can
// never be `Loud.speak`'s super target. scalac 2.13.16: "`abstract override`
// modifiers required to override".
object Main {
  abstract class Animal { def speak: String }
  trait Loud extends Animal { abstract override def speak = "LOUD-" + super.speak }
  class Own extends Animal with Loud { override def speak = "own" }
  def main(args: Array[String]): Unit = println(new Own().speak)
}
