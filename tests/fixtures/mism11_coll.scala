// Two library hierarchy gaps, both of which type-checked into something the
// verifier rejects or refused a call that is correct.
//
//  * `it.grouped(n)` is an `Iterator.GroupedIterator[B]`, whose element type is
//    `Seq[B]` -- it is an `AbstractIterator[Seq[B]]`. Its inherited `map` was
//    read from the pickle with the class's `B` captured by `map`'s own binder,
//    and the lambda was then typed against the receiver's first type argument
//    anyway, so `case Seq(i, t) => …` destructured an `Int`.
//  * `mutable.ArrayBuilder.make[E]` is a `Builder[E, Array[E]]`. The class
//    file can only say `ReusableBuilder<T, Object>` in its generic signature,
//    and `To` is invariant, so returning one as a `Builder[E, Array[E]]` was
//    `found: ArrayBuilder[E]  required: Builder[E, Array[E]]`.

import scala.collection.mutable
import scala.reflect.ClassTag

object Main {
  def createBuilder[E: ClassTag]: mutable.Builder[E, Array[E]] = mutable.ArrayBuilder.make[E]

  def main(args: Array[String]): Unit = {
    val clauses = Seq(1, 2, 3, 4, 5, 6)
    val pairs = clauses.iterator.grouped(2).withPartial(false).map { case Seq(i, t) => (i, t) }
    println(pairs.toList)

    val lens = Seq("a", "bb", "ccc").iterator.grouped(2).map(g => g.map(_.length).sum)
    println(lens.toList)

    val b = createBuilder[Int]
    b += 1
    b += 2
    b += 3
    println(b.result().toList)

    val s = createBuilder[String]
    s += "x"
    s += "y"
    println(s.result().mkString("-"))
  }
}
