// cats' `Newtype` encoding, but with the abstract type member genuinely
// *inherited*, not redeclared -- unlike `nel_newtype.scala`'s `Widget`
// object, which declares its own `type Type[+A] <: Base with Tag` directly.
// Here the member lives once on a shared `Newtype` trait, and `WidgetImpl`
// below `extends Newtype` without ever overriding it -- the actual cats
// shape (`object NonEmptySetImpl extends NonEmptySetInstances with
// Newtype`), and the one `docs/cats.md`'s "`Type::TypeMember` has no
// prefix" note is about.
//
// `type Widget[A] = WidgetImpl.Type[A]`, inherited by the package object
// from `DataVersionSpecific` (not declared in the package object's own
// body, the same indirection `nel_newtype.scala` uses to keep the object
// and the alias in genuinely different namer scopes), never overrides
// `Type` either -- it is a plain alias to the abstract member. Reading
// `value: Widget[A]` (a parameter of `WidgetOps`'s own conversion) only
// ever found `Type`'s upper bound's class (`Base`'s companion, and `Base`
// here has none), because `Type::TypeMember` carries just the defining
// symbol, never the prefix (`WidgetImpl.type`) the source actually
// selected `Type` through -- so the conversion `WidgetImpl` itself
// declares was invisible to implicit search, and every method `WidgetOps`
// adds reported "value ... is not a member of Newtype.Type[A]".
package tm {
  package data {
    import scala.language.implicitConversions

    private[data] trait Newtype {
      private[data] type Base
      private[data] trait Tag extends Any
      type Type[A] <: Base with Tag
    }

    object WidgetImpl extends Newtype {
      private[data] def create[A](s: List[A]): Type[A] = s.asInstanceOf[Type[A]]
      private[data] def unwrap[A](w: Type[A]): List[A] = w.asInstanceOf[List[A]]

      def of[A](as: List[A]): Type[A] = create(as)

      implicit def widgetOps[A](value: Widget[A]): WidgetOps[A] = new WidgetOps(value)
    }

    sealed class WidgetOps[A](private[data] val value: Widget[A]) {
      def toList: List[A] = WidgetImpl.unwrap(value)
      def size: Int = toList.size
      def prepended(a: A): Widget[A] = WidgetImpl.create(a :: toList)
    }

    abstract private[data] class DataVersionSpecific {
      type Widget[A] = WidgetImpl.Type[A]
    }
  }

  package object data extends data.DataVersionSpecific
}

object Main {
  def main(args: Array[String]): Unit = {
    val w: tm.data.Widget[Int] = tm.data.WidgetImpl.of(1 :: 2 :: 3 :: Nil)
    println(w.size)
    val w2 = w.prepended(0)
    println(w2.toList.mkString(","))
  }
}
