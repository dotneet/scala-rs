// 埋まらない implicit 節は、修飾子位置でも導出規則の中でも「診断」であること。
// 修正が「黙って通す」方向へ緩んでいないかを押さえる。

import scala.reflect.ClassTag

trait Sh[-M, P]
final case class SV[A, B](value: A, shape: B)
final class Qy[E](val name: String) {
  def pack[R](implicit packing: Sh[E, R]): Qy[R] = new Qy[R](name)
  def to[D[_]]: Qy[E] = this
}

trait Coll[C[_]]
object Coll {
  // `Sh` の witness は 1 つも無い。`ClassTag` が埋まることは、この規則を
  // 使えることを意味しない。
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
