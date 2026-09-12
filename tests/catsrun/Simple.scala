// The smallest client that touches `cats.implicits`, kept as its own program.
//
// Loading `cats.implicits$` runs the initializers of every instance trait it
// mixes in, so this three-line program is what first executes
// `cats.kernel.instances.*`, `cats.instances.*` and `cats.package$`. Four
// separate miscompilations showed up here before any of the larger clients
// below could run a single line, and each one failed during *class
// initialization*: a defect anywhere in that graph takes out every cats
// program at once. Keep it first and keep it small.
import cats._
import cats.implicits._

object Main {
  def main(args: Array[String]): Unit = {
    println(Functor[List].map(List(1, 2, 3))(_ + 1))
    println(Monoid[Int].combine(2, 3))
    println(Foldable[List].foldMap(List(1, 2, 3))(_.toString))
  }
}
