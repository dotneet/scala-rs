// `TypeTag` / `WeakTypeTag` materialisation (`docs/macros.md` §7.10).
//
// None of the tags below is written down anywhere: `typeOf[T]` asks for an
// implicit `TypeTag[T]` and the compiler has to *build* one, out of a
// `TypeCreator` that rebuilds `T` inside the mirror it is handed. Every line
// prints what the built tag says the type is, and this file is run under real
// scalac 2.13.16 as well -- the two must agree exactly.

import scala.reflect.runtime.universe._

class Foo
trait Bar
class Baz(val n: Int)

object Main {
  def show(t: Type): String = t.toString

  def main(args: Array[String]): Unit = {
    // A class, a trait, and a class with members.
    println(show(typeOf[Foo]))
    println(show(typeOf[Bar]))
    println(show(typeOf[Baz]))

    // Every primitive, plus `Unit` and `String`.
    println(show(typeOf[Boolean]))
    println(show(typeOf[Byte]))
    println(show(typeOf[Short]))
    println(show(typeOf[Char]))
    println(show(typeOf[Int]))
    println(show(typeOf[Long]))
    println(show(typeOf[Float]))
    println(show(typeOf[Double]))
    println(show(typeOf[Unit]))
    println(show(typeOf[String]))

    // The top and bottom of the hierarchy.
    println(show(typeOf[Any]))
    println(show(typeOf[AnyVal]))
    println(show(typeOf[Nothing]))
    println(show(typeOf[Null]))

    // A library class the program never names otherwise.
    println(show(typeOf[scala.math.BigInt]))

    // `weakTypeOf` takes a `WeakTypeTag`; for a type this concrete nsc and
    // scala-rs both hand it a tag of the very same type.
    println(show(weakTypeOf[Foo]))
    println(show(weakTypeOf[Int]))

    // The tags themselves, not just `typeOf`.
    println(show(typeTag[Bar].tpe))
    println(show(weakTypeTag[Baz].tpe))
    // `implicitly[TypeTag[Foo]]` is *not* here: naming the tag's type at all
    // -- `TypeTag[Foo]`, `u.TypeTag[Foo]` -- still fails, and for a reason
    // that has nothing to do with materialisation (`docs/macros.md` §7.10,
    // residual 2). `typeTag[Foo]` above asks for exactly the same implicit.

    // Two independently materialised tags describe the same type.
    println(typeOf[Foo] =:= typeOf[Foo])
    println(typeOf[Foo] =:= typeOf[Bar])
    println(typeOf[Foo] <:< typeOf[Any])
    println(typeOf[Nothing] <:< typeOf[Foo])
    println(typeOf[String] =:= typeOf[java.lang.String])

    // What the tag knows about the class it stands for.
    println(typeOf[Baz].typeSymbol.name.toString)
    println(typeOf[Foo].typeSymbol.fullName)
    println(typeOf[scala.math.BigInt].typeSymbol.fullName)
  }
}
