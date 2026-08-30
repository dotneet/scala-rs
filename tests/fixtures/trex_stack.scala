// SLS 5.3.3 / 5.1.2: a trait may extend a class. The parent is a *constraint*
// -- the trait never runs `Animal`'s constructor -- and `abstract override`
// resolves `super` along the linearization of whatever concrete class mixes
// the trait in, so the order of the `with` clauses changes the result.
//
// Only string concatenation is used, so this runs under the private runtime
// as well as against the real scala-library jar.
object Main {
  abstract class Animal(val name: String) {
    def speak: String
    def greet: String = "hi " + name
    override def toString = name + " says " + speak
  }

  trait Loud extends Animal {
    abstract override def speak = "LOUD-" + super.speak
    // A trait body may also read members inherited from its superclass.
    def loudGreet: String = greet + "!" + name
  }

  trait Polite extends Animal {
    abstract override def speak = "please-" + super.speak
  }

  trait Twice extends Animal {
    abstract override def speak = super.speak + " " + super.speak
  }

  // A trait may extend a trait that carries the constraint.
  trait Sub extends Loud

  class Dog extends Animal("Rex") { def speak = "woof" }
  class LoudDog extends Dog with Loud
  class SubDog extends Dog with Sub

  def main(args: Array[String]): Unit = {
    println(new Dog)
    println(new Dog with Loud)
    println(new Dog with Twice with Loud)
    println(new Dog with Loud with Twice)
    // Linearization order really does change the answer here.
    println(new Dog with Polite with Loud)
    println(new Dog with Loud with Polite)
    println(new LoudDog)
    println(new LoudDog().loudGreet)
    println(new SubDog)
    val a: Animal = new LoudDog
    println(a.speak)
  }
}
