// Type tags the materialiser builds through the type reifier
// (`docs/notes/reify-design.md`): a class nested in an object, an alias, a
// singleton, a type parameter with no tag (a free type, so a `WeakTypeTag`),
// and `Predef.String` spelled as the alias it is. `crates/cli/tests/reify2.rs`
// compares every line with real scalac 2.13.16.
import scala.reflect.runtime.universe._

class Foo
object Nest {
  class Inner
  object Deep { class Deeper; object O }
}

object Main extends App {
  object Bar
  class Baz

  println(typeOf[List[Nest.Inner]])
  println(weakTypeOf[(Int, Nest.Inner)])
  println(typeOf[Nest.Inner])
  println(typeOf[Nest.Deep.Deeper])
  println(typeOf[Nest.Deep.O.type])
  println(typeOf[AnyRef])
  println(typeOf[AnyRef] =:= typeOf[Object])
  println(typeOf[Main.type])
  println(typeOf[Bar.type])
  println(typeOf[Baz])
  println(typeOf[Nest.type].typeSymbol.fullName)
  def g[T]: Type = weakTypeOf[T]
  println(g[Int].typeSymbol.isAbstract)
  println(g[Int].typeSymbol.name)
  // A concrete type answers a `WeakTypeTag` request with a `TypeTag`.
  println(implicitly[WeakTypeTag[Int]])
  println(implicitly[WeakTypeTag[List[Int]]])
  println(showRaw(typeOf[String]))
  println(typeOf[String] =:= typeOf[java.lang.String])
  println(typeOf[Array[Int]])
  println(typeOf[(Int, String) => Foo])
}
