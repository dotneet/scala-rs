package inheritedbridgeowner

class Impl extends Traverse

object Main extends App {
  val foldable: Foldable = new Impl
  val invariant: Invariant = new Impl
  println(foldable.compose(new Impl).getClass.getName)
  println(invariant.compose(new Impl).getClass.getName)
}
