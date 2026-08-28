package cake.badbase

trait Component { self: Base =>
  class Present[T](val n: String)
}

/** Declared, but never mixed into `Base`. */
trait DetachedComponent { self: Base =>
  class Detached[T](val n: String)
}

trait Base extends Component
