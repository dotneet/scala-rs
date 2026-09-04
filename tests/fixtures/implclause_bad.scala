// An implicit clause that cannot be filled must be a diagnostic, both in qualifier
// position and inside a derivation rule. Pins that the fix did not slacken into
// quietly accepting.

import scala.reflect.ClassTag

trait Sh[-M, P]
final case class SV[A, B](value: A, shape: B)
final class Qy[E](val name: String) {
  def pack[R](implicit packing: Sh[E, R]): Qy[R] = new Qy[R](name)
  def to[D[_]]: Qy[E] = this
}

trait Coll[C[_]]
object Coll {
  // There is not a single `Sh` witness. That a `ClassTag` can be filled does not
  // mean this rule is usable.
  implicit def forColl[C[_]](implicit s: Sh[C[Any], C[Any]], tag: ClassTag[C[Any]]): Coll[C] =
    new Coll[C] {}
}

object Main {
  def main(args: Array[String]): Unit = {
    val q = new Qy[Int]("q")
    println(SV(q.pack.to[Seq], "x"))
    println(implicitly[Coll[Seq]])
  }
}
