// scalac 2.13.16 reports one `not found: type X` per unresolvable name in a
// template header -- the superclass, each `with` item, the head of an applied
// parent and a type argument inside one are all separate reports, pointed at
// the name itself. All of these used to compile without a word and emit a
// class file extending `java/lang/Object`.
object Bogus extends NoSuchThingHere

class C extends AlsoMissing

trait T extends MissingTrait

class D extends Object with MissingMixin

class E extends MissingGen[Int]

trait Holder[A]
class F extends Holder[MissingArg]

object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}
