// Every parent shape the "unknown parent" check has to leave alone: a class
// with constructor arguments, a generic parent, mixins, a self type, an
// anonymous class, a qualified parent name and a parent reached through a
// type alias. Each of these once went through the same `Type::Named`
// placeholder an unresolvable name does, so a check that fires on the
// placeholder alone would reject all of them.
object Outer {
  trait Named { def name: String }
  abstract class Animal(val id: Int) extends Named { def speak: String = "base" }
  trait Loud extends Animal { abstract override def speak = "LOUD-" + super.speak }
  trait Box[A] { def get: A }
  class Dog(id: Int) extends Animal(id) with Loud { def name = "dog" }
  class IntBox extends Box[Int] { def get = 7 }
  type Beast = Animal
}

trait NeedsName { self: Outer.Named => def shout: String = name + "!" }

object Main {
  import Outer._

  class Cat(id: Int) extends Outer.Animal(id) with NeedsName {
    def name = "cat"
    override def speak = "meow"
  }

  class Alias extends Beast(9) { def name = "alias" }

  def main(args: Array[String]): Unit = {
    val d = new Dog(1)
    println(d.speak + " " + d.id + " " + d.name)
    println(new IntBox().get)
    val c = new Cat(2)
    println(c.speak + " " + c.shout)
    println(new Alias().name + " " + new Alias().speak)
    val anon = new Box[String] { def get = "anon" }
    println(anon.get)
    val anon2 = new Outer.Animal(3) with Loud {
      def name = "anon2"
    }
    println(anon2.speak + " " + anon2.id)
  }
}
