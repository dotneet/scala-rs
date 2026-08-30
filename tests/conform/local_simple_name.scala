// `getSimpleName` on a local class.
//
// nsc disambiguates local declarations with a `$N` suffix and puts that
// suffix in the `InnerClasses` `inner_name` field as well as in the binary
// name -- `getSimpleName` reads the field, so writing the undecorated name
// there gave `Dog` where scalac gives `Dog$1`.
object Main {
  def main(args: Array[String]): Unit = {
    abstract class Animal {
      def name: String
      def sound: String
      final def speak: String = s"$name says $sound"
    }
    class Dog(val name: String) extends Animal { def sound = "woof" }
    class Cat(val name: String) extends Animal { def sound = "meow" }

    val as: List[Animal] = List(new Dog("rex"), new Cat("tom"))
    as.foreach(x => println(x.speak))
    println(as.map(_.getClass.getSimpleName))
    println(as.head.getClass.getName)
    println(as.head.getClass.isMemberClass, as.head.getClass.isLocalClass)
  }
}
