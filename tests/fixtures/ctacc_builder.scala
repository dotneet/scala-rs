// `scala.collection.mutable.Builder` is not a prelude class: it arrives
// through its pickle. `+=` and `++=` are `Growable`'s default methods and
// return `this.type`, which the pickle supplier used to decline to map -- so
// `b ++= xs` was mistaken for an assignment (`b = b ++ xs`) and reported as an
// unassignable receiver, even though the member is right there.
//
// Library-ABI only: the private runtime has no `Growable` at all.

import scala.collection.mutable.Builder

class ListB extends Builder[Int, List[Int]] {
  private var acc: List[Int] = Nil
  def addOne(e: Int): this.type = {
    acc = e :: acc
    this
  }
  def clear(): Unit = {
    acc = Nil
  }
  def result(): List[Int] = acc.reverse
}

object Main {
  // The shape slick's `DBIOAction.sequence` uses: the result of `++=` is the
  // builder again, so `result()` can be called on it.
  def fill(b: Builder[Int, List[Int]]): List[Int] = {
    b += 1
    val same = b ++= List(2, 3)
    same += 4
    same.result()
  }

  def main(args: Array[String]): Unit = {
    println(fill(new ListB))
  }
}
