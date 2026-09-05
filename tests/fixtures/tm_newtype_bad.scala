// The `Newtype` implicit-scope fix (see tm_newtype.scala) must not become
// permissive about arity: `Widget` still takes exactly one type parameter,
// and nsc rejects `Widget[Int, String]` too ("wrong number of type arguments
// for tm.data.Widget, should be 1").
package tm {
  package data {
    private[data] trait Newtype {
      private[data] type Base
      private[data] trait Tag extends Any
      type Type[A] <: Base with Tag
    }

    object WidgetImpl extends Newtype

    abstract private[data] class DataVersionSpecific {
      type Widget[A] = WidgetImpl.Type[A]
    }
  }

  package object data extends data.DataVersionSpecific
}

object Main {
  def bad(w: tm.data.Widget[Int, String]): Unit = ()
}
