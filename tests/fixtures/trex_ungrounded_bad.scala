// `Dog2.speak` stays abstract, so `Loud.speak`'s `super.speak` has nothing to
// call. scalac 2.13.16: `object creation impossible.` plus "is marked
// `abstract` and `override`, but no concrete implementation could be found in
// a base class".
object Main {
  abstract class Animal { def speak: String }
  trait Loud extends Animal { abstract override def speak = "LOUD-" + super.speak }
  abstract class Dog2 extends Animal
  def main(args: Array[String]): Unit = {
    println((new Dog2 with Loud).speak)
  }
}
