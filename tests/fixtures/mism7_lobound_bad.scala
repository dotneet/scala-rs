// `O >: E` makes `E <: O`, not the other way round.
object Main {
  def wrong[E, O >: E](x: O): E = x
  def main(args: Array[String]): Unit = ()
}
