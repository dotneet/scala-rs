// Sixth `type mismatch` slice, plus the three codegen bugs recorded with it.
//
// Everything here runs on both the private runtime and the real
// scala-library, so it names only `List`, `Option`, `Tuple2` and `String` --
// and prints scalars, because the private runtime's classes do not override
// `toString`.

object Main {
  class Holder {
    var cur: Option[Int] = None
    var xs: List[Int] = Nil
  }

  // A `match` whose branches push different classes (`Some` and `None$`),
  // stored into a field. The frame that merges them says `Option`, so the
  // `putfield` needs no cast it never got.
  def pick(n: Int): Option[Int] = n match {
    case 0 => None
    case k => Some(k)
  }

  // The same for `if`.
  def iff(n: Int): Option[Int] = if (n == 0) None else Some(n)

  def show(o: Option[Int]): Int = o.getOrElse(-1)

  // A lambda whose parameter type is written out still has its body checked
  // against the expected *result*: discarded here, widened below.
  def each(xs: List[Int]): Unit = xs.foreach((x: Int) => x + 1)
  def wide(f: Int => Long): Long = f(3)

  // A value definition is the last enumerator, so the generator before it
  // takes `map`, not `flatMap`.
  def names(ms: List[Int]): List[String] = for {
    m <- ms
    if m > 0
    q = "n" + m.toString
  } yield q

  // `withFilter(...).map(f)` has the element type `f` returns.
  def filtered(ms: List[Int]): List[String] = ms.withFilter(_ > 1).map(_.toString)

  // The merge type is the expression's own, at every depth: the inner `match`
  // here is an argument, and its branches are a `String` and an `Int`.
  def nested(n: Int): Option[Any] =
    if (n == 0) None else Some(n match { case 1 => "one"; case _ => n })

  // A `try` whose handler does not conform to its body is the lub of the two,
  // and the result is boxed into the one slot they share.
  def tryMix(n: Int): Any = try n catch { case _: Exception => "x" }
  def tryOpt(n: Int): Option[Int] = try Some(n) catch { case _: Exception => None }

  def join(ss: List[String]): String = {
    var acc = ""
    ss.foreach((s: String) => acc = acc + s + ";")
    acc
  }

  def main(args: Array[String]): Unit = {
    val h = new Holder
    h.cur = (3: Int) match { case 0 => None; case n => Some(n) }
    println(show(h.cur))
    h.cur = if (h.cur.isEmpty) Some(1) else None
    println(show(h.cur))
    h.xs = (1: Int) match { case 1 => Nil; case _ => 1 :: Nil }
    println(h.xs.isEmpty)
    println(show((7: Int) match { case 0 => None; case n => Some(n) }))
    println(show(pick(0)))
    println(show(pick(5)))
    println(show(iff(0)))
    println(show(iff(9)))

    // A pattern definition whose components carry a type ascription.
    val b = true
    val (n: Int, s: String) = if (b) (1, "x") else (0, "y")
    println(n)
    println(s)

    each(1 :: 2 :: Nil)
    println(wide((x: Int) => x + 1))
    println(join(names(1 :: 2 :: 0 :: Nil)))
    println(join(filtered(1 :: 2 :: 3 :: Nil)))
    println(nested(0).isEmpty)
    println(nested(1).getOrElse("?").toString)
    println(nested(7).getOrElse("?").toString)
    println(tryMix(5).toString)
    println(show(tryOpt(6)))
  }
}
