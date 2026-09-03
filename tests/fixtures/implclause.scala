// implicit 引数節が適用されないまま式の型に残るバグの回帰テスト。
// 4 つの根を 1 ファイルにまとめてある（実 scalac 2.13.16 で同じ出力）。

import scala.collection.Factory
import scala.reflect.ClassTag

// (1) 関数パラメータの *結果* を、パラメータ側のクラスに揃えてから型引数を解く。
//     `flatMap[B](f: A => IterableOnce[B])` にラムダの本体が `Map[K, V]` を返す
//     ものを渡すと、`[B]` と `[K, V]` を位置で zip して `B = K` と解いていた。
//     結果 `toMap` の `A <:< (K, V)` が見つからず、
//     `(<:<[K, (K, V)])Map[K, V]` が式の型として残っていた。
object Root1 {
  def collect(mapped: Vector[(String, Map[Long, Int])]): Map[Long, Int] =
    mapped.iterator.flatMap(_._2).toMap
  def isEmptyOf(mapped: Vector[(String, Map[Long, Int])]): Boolean =
    mapped.iterator.flatMap(_._2).toMap.isEmpty
}

// (2) `A => B` を *継承* したクラス（`<:<` もそう）を Function1 パラメータへ
//     渡したとき、呼び先の型引数がその引数から解けていなかった。適合自体は
//     通るので `val g: R => S = ev` は書けるのに、`flatMap(ev)` は
//     「no matching overload」になっていた。
abstract class Conv[-A, +B] extends (A => B)
final class Act[+R](val value: R) {
  def flatMap[R2](f: R => Act[R2]): Act[R2] = f(value)
  def flatten[R2](implicit ev: R <:< Act[R2]): Act[R2] = flatMap(ev)
}
object Root2 {
  def viaConv[A, B](ev: Conv[A, B], a: A): B = {
    def id[X, Y](f: X => Y): X => Y = f
    id(ev)(a)
  }
  val upper: Conv[String, String] = new Conv[String, String] {
    def apply(s: String) = s.toUpperCase
  }
}

// (3) セレクションの *修飾子* は、それが呼び出し引数の中にあっても
//     implicit 節を埋めてから使う。`SV(pack.to[Seq], "x")` の `pack` が
//     `(Sh[…])Qy[R]` のまま残り、`to` が「not a member」になっていた。
trait Sh[-M, P]
final case class SV[A, B](value: A, shape: B)
final class Qy[E](val name: String) {
  def pack[R](implicit packing: Sh[E, R]): Qy[R] = new Qy[R](name + "+p")
  def to[D[_]]: Qy[E] = new Qy[E](name + "+t")
}
object Root3 {
  implicit def idSh[T]: Sh[T, T] = new Sh[T, T] {}
  def wrap(q: Qy[Int]): SV[Qy[Int], String] = SV(q.pack.to[Seq], "x")
}

// (4) 導出規則が自分の implicit 引数に `ClassTag` を持つとき、その規則は
//     「使えない候補」として捨てられていた。`ClassTag` は探索ではなく生成で
//     埋まるもので、`implicitly[ClassTag[Seq[Any]]]` 単体は通っていた。
trait Coll[C[_]] { def name: String }
object Coll {
  implicit def forColl[C[X] <: Iterable[X]](implicit
      cbf: Factory[Any, C[Any]],
      tag: ClassTag[C[Any]]
  ): Coll[C] = new Coll[C] { def name = "coll" }
}

object Main {
  def main(args: Array[String]): Unit = {
    val v = Vector(("a", Map(1L -> 10)), ("b", Map(2L -> 20)))
    println(Root1.collect(v).toSeq.sortBy(_._1))
    println(Root1.isEmptyOf(v))
    println(Root1.isEmptyOf(Vector()))
    println(Root2.viaConv(Root2.upper, "hi"))
    println(new Act(new Act(7)).flatten.value)
    println(Root3.wrap(new Qy[Int]("q")).value.name)
    println(implicitly[Coll[Vector]].name)
    println(implicitly[Coll[Seq]].name)
  }
}
