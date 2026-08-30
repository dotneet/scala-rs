// A `private` trait method read from the trait's own companion object: the
// typer widens it (JVM has no cross-class `private`), and it must still get
// a real interface signature -- unlike the truly-private case in `tp1` /
// `tp2`, this one is *not* skipped.
trait Widened {
  private def secret: Int = 42
}
object Widened {
  def reveal(w: Widened): Int = w.secret
}
class WidenedImpl extends Widened

object Main {
  def main(args: Array[String]): Unit = {
    println(Widened.reveal(new WidenedImpl))
  }
}
