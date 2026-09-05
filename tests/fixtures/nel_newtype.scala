// cats' `Newtype` encoding: `object NonEmptyLazyList` declares its own
// `type Type[+A] <: Base with Tag` directly, and a *different* file's package
// object exports `type NonEmptyLazyList[+A] = NonEmptyLazyList.Type[A]` --
// naming the object and the alias the same, in two different namespaces. See
// docs/cats.md.
//
// Reduced to one file with an explicit `package nel { package data { ... } }`
// nesting so the object `Widget` and the alias-bearing package object are
// still declared in genuinely different scopes (as `NonEmptyLazyList.scala`
// and its package's `package.scala` are for cats), which is what exposed the
// bug: a bare `Widget[A]`, used inside `Widget`'s own file, has to resolve to
// the alias's target arity, not to the object (kind arity 0).
package nel {
  package data {
    import scala.language.implicitConversions

    object Widget {
      private[data] type Base
      private[data] trait Tag extends Any
      type Type[+A] <: Base with Tag

      private[data] def create[A](s: List[A]): Type[A] = s.asInstanceOf[Type[A]]
      private[data] def unwrap[A](s: Type[A]): List[A] = s.asInstanceOf[List[A]]

      def of[A](as: List[A]): Type[A] = create(as)

      // `Widget[A]` here is the bare alias name, used before the alias
      // itself (declared below, on `DataVersionSpecific`) is folded into
      // this package -- "Widget does not take type parameters" was reported
      // at exactly this line.
      implicit def widgetOps[A](value: Widget[A]): WidgetOps[A] = new WidgetOps(value)
    }

    sealed class WidgetOps[A](private[data] val value: Widget[A]) {
      def toList: List[A] = Widget.unwrap(value)
      def size: Int = toList.size
      def prepended(a: A): Widget[A] = Widget.create(a :: toList)
    }

    // The alias is inherited from this parent, not declared in the package
    // object's own body -- cats' `package object data extends
    // ScalaVersionSpecificPackage` is exactly this shape, and folding only a
    // package object's *direct* members (not what it inherits) left the
    // alias unreachable from any other file.
    abstract private[data] class DataVersionSpecific {
      type Widget[A] = Widget.Type[A]
    }
  }

  package object data extends data.DataVersionSpecific
}

object Main {
  def main(args: Array[String]): Unit = {
    val w: nel.data.Widget[Int] = nel.data.Widget.of(1 :: 2 :: 3 :: Nil)
    println(w.size)
    val w2 = w.prepended(0)
    println(w2.toList.mkString(","))
  }
}
