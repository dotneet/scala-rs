package hkemptiness

trait Emptiness[-T] {
  def label: String
}

object Instances {
  implicit def iterable[E, TRAV[e] <: Iterable[e]]: Emptiness[TRAV[E]] =
    new Emptiness[TRAV[E]] { def label = "iterable" }

  implicit def option[E, OPT[e] <: Option[e]]: Emptiness[OPT[E]] =
    new Emptiness[OPT[E]] { def label = "option" }

  implicit def javaCollection[E, JCOL[e] <: java.util.Collection[e]]: Emptiness[JCOL[E]] =
    new Emptiness[JCOL[E]] { def label = "java" }

  implicit def hasIsEmpty[T <: AnyRef { def isEmpty(): Boolean }]: Emptiness[T] =
    new Emptiness[T] { def label = "structural-parens" }

  implicit def hasParameterlessIsEmpty[T <: AnyRef { def isEmpty: Boolean }]: Emptiness[T] =
    new Emptiness[T] { def label = "structural" }
}

object OnlyIterable {
  implicit def iterable[E, TRAV[e] <: Iterable[e]]: Emptiness[TRAV[E]] =
    new Emptiness[TRAV[E]] { def label = "wrong" }
}
