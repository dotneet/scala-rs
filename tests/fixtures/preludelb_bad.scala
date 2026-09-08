// `Map.updated` was `(key: Any, value: Any): Map[K, V]`, so the widening
// never happened and the result kept the receiver's own value type. The
// branch point compiled this file with no diagnostic at all.
//
// Nothing weaker catches it. `Map` is covariant in `V`, so
// `val m: Map[String, Animal] = md.updated("k", cat)` is accepted whether the
// widening happened or not -- `Map[String, Dog]` conforms too. Only forcing
// the *narrow* type separates the two.
class Animal(val name: String)
class Dog(name: String) extends Animal(name)
class Cat(name: String) extends Animal(name)

object Main {
  val md: Map[String, Dog] = Map("a" -> new Dog("rex"))
  val cat: Cat = new Cat("tom")
  val narrow: Dog = md.updated("b", cat).apply("b")
}
