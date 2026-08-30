// An `object` is an instance too: scalac 2.13.16 reports the same
// `object creation impossible.` here as it does at a `new C with T`.
object Main {
  abstract class Animal { def speak: String }
  trait Loud extends Animal { abstract override def speak = "LOUD-" + super.speak }
  object O extends Animal with Loud
  def main(args: Array[String]): Unit = println(O.speak)
}
