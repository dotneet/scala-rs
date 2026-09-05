// The collections library's infix cons operators and extractor objects:
// `+:` / `:+` (`scala.collection.package$$plus$colon$` and its sibling) in
// pattern position, and `#::` / `#:::` in both positions, for `LazyList` and
// for `Stream`.
//
// The container a `+:` pattern binds has to be the scrutinee's own -- an
// `ArraySeq` stays an `ArraySeq`, a `LazyList` stays a `LazyList` -- which is
// what `unapply[A, C <: Seq[A]]` is for. `sameArraySeq` and `lazyTail` would
// not compile if it yielded a plain `Seq`.
//
// `ones` is the case that says the tail is not forced.
import scala.collection.immutable.ArraySeq

object Main {
  def headTail(xs: Seq[Int]): String = xs match {
    case h +: t => h.toString + "/" + headTail(t)
    case _      => "."
  }

  def initLast(xs: Vector[Int]): String = xs match {
    case t :+ h => initLast(t) + "\\" + h.toString
    case _      => "."
  }

  def sameArraySeq(xs: ArraySeq[Int]): ArraySeq[Int] = xs match {
    case _ +: rest => rest
    case _         => xs
  }

  def lazyTail(xs: LazyList[Int]): LazyList[Int] = xs match {
    case _ +: rest => rest
    case _         => xs
  }

  def sumLazy(xs: LazyList[Int]): Int = xs match {
    case v #:: t => v + sumLazy(t)
    case _       => 0
  }

  def digitsOfStream(xs: Stream[Int]): Int = xs match {
    case v #:: t => v * 10 + digitsOfStream(t)
    case _       => 0
  }

  def consLazy(a: Int, xs: LazyList[Int]): LazyList[Int] = a #:: xs
  def concatLazy(xs: LazyList[Int], ys: LazyList[Int]): LazyList[Int] = xs #::: ys
  def consStream(a: Int, xs: Stream[Int]): Stream[Int] = a #:: xs
  def concatStream(xs: Stream[Int], ys: Stream[Int]): Stream[Int] = xs #::: ys

  // Neither the head nor the tail may be forced: a strict `#::` never returns.
  def ones: LazyList[Int] = 1 #:: ones

  def main(args: Array[String]): Unit = {
    println(headTail(Seq(1, 2, 3)))
    println(initLast(Vector(1, 2, 3)))
    println(sameArraySeq(ArraySeq.unsafeWrapArray(Array(1, 2, 3))).mkString(","))
    println(lazyTail(LazyList(1, 2, 3)).mkString(","))
    println(sumLazy(LazyList(1, 2, 3)))
    println(digitsOfStream(Stream.range(1, 4)))
    println(consLazy(0, LazyList(1, 2)).mkString(","))
    println(concatLazy(LazyList(1, 2), LazyList(3)).mkString(","))
    println(consStream(0, Stream.range(1, 3)).mkString(","))
    println(concatStream(Stream.range(1, 3), Stream.range(5, 7)).mkString(","))
    println(ones.take(4).mkString(","))
  }
}
