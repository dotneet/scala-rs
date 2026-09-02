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

    // A type constructor at its arguments. One `staticClass` call cannot
    // rebuild these: the creator composes `appliedType` over the class and
    // each argument's own body (`docs/macros.md` §7.12). Until that landed
    // every one of them was refused by name, and `tt_tags_bad.scala` still
    // pins the arguments that cannot be composed.
    println(show(typeOf[List[Int]]))
    println(show(weakTypeOf[Option[Foo]]))
    // `Map` is `Predef.Map`, an *alias*: nsc's creator keeps the alias
    // (`selectType(staticModule("scala.Predef"), "Map")`), scala-rs's names
    // the class it points at. The two types are `=:=` and have the same
    // symbol; only `toString` differs, so that is what is compared here --
    // the same deviation the `String` case above already carries.
    println(typeOf[Map[String, Foo]] =:= typeOf[scala.collection.immutable.Map[String, Foo]])
    println(typeOf[Map[String, Foo]].typeSymbol.fullName)
    println(show(typeOf[List[List[Int]]]))
    println(typeOf[List[Int]] =:= typeOf[List[Int]])
    println(typeOf[List[Int]] =:= typeOf[List[String]])
    println(typeOf[List[Int]] <:< typeOf[Any])
    println(typeOf[List[Int]].typeSymbol.fullName)

    // A tuple, a function type and an array. Each is a `Type` of its own in
    // scala-rs and an ordinary `TypeRef` in reflect, so the creator names
    // `scala.TupleN` / `scala.FunctionN` / `scala.Array` and composes the
    // arguments the same way (`docs/macros.md` §7.13). slick's
    // `TableQueryMacroImpl` asks for exactly the function one --
    // `c.Expr[Tag => E]` -- and was stopped here.
    println(show(weakTypeOf[(Int, Foo)]))
    println(show(typeOf[Int => Foo]))
    println(show(typeOf[(Int, String) => Foo]))
    println(show(typeOf[Array[Int]]))
    println(show(typeOf[List[(Int, Foo)]]))
    println(typeOf[Int => Foo] =:= typeOf[Function1[Int, Foo]])
    println(typeOf[(Int, Foo)].typeSymbol.fullName)
    println(typeOf[Int => Foo].typeSymbol.fullName)
  }
}
