// The `Newtype` alias resolution fix (see nel_newtype.scala) must not become
// permissive about arity: `Widget` still takes exactly one type parameter,
// and nsc rejects `Widget[Int, String]` too ("wrong number of type arguments
// for nel.data.Widget, should be 1").
package nel {
  package data {
    object Widget {
      private[data] type Base
      private[data] trait Tag extends Any
      type Type[+A] <: Base with Tag
    }

    abstract private[data] class DataVersionSpecific {
      type Widget[A] = Widget.Type[A]
    }
  }

  package object data extends data.DataVersionSpecific
}

object Main {
  def bad(w: nel.data.Widget[Int, String]): Unit = ()
}
