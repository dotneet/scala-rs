package inheritedbridgeowner

trait Invariant {
  def compose(other: Invariant): Invariant = other
}

trait Functor extends Invariant {
  def compose(other: Functor): Functor = other
}

trait Foldable {
  def compose(other: Foldable): Foldable = other
}

trait Traverse extends Functor with Foldable {
  def compose(other: Traverse): Traverse = other
}
