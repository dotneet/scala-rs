// An implicit member of an enclosing class is selected on that class's
// `this`, not through a companion object that inherits it: the companion's
// `MODULE$` is still null while its parent's constructor runs.
package outerimpl

trait Fu[F[_]] { def id: Int }
object Fu { def apply[F[_]](implicit f: Fu[F]): Fu[F] = f }
trait Holder { val F: Fu[Ev] }

sealed abstract class Ev[A]
object Ev extends EvInstances

sealed abstract class EvInstances {
  implicit val fuEv: Fu[Ev] = new Fu[Ev] { def id = 1 }
  // The instance is built in the constructor that `object Ev` runs first.
  implicit val holder: Holder = new Holder { val F: Fu[Ev] = Fu[Ev] }
  // Two classes deep.
  class Outer { class Inner { def id: Int = Fu[Ev].id + 1 } }
  val outer = new Outer
  val inner: Int = new outer.Inner().id
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Ev.holder.F.id)
    println(Ev.inner)
  }
}
