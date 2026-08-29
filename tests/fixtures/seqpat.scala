// Sequence patterns on `Seq` / `List` / `Vector` / `IndexedSeq` / `Array`:
// fixed arity, `_*`, nested patterns and tuple elements.
case class P(x: Int, y: Int)
class Nd
class TableNd extends Nd

object Main {
  def seqShape(v: Seq[Int]): String = v match {
    case Seq() => "empty"
    case Seq(a) => "one " + a
    case Seq(a, b) => "two " + (a + b)
    case Seq(a, b, rest @ _*) => "many " + (a + b) + " " + rest.size
  }

  def listShape(xs: List[String]): String = xs match {
    case List(a, b, rest @ _*) => a + b + rest.mkString("|")
    case List(a) => a
    case _ => "?"
  }

  def vecShape(v: Vector[String]): String = v match {
    case Vector(a, b) => a + b
    case Vector(a, rest @ _*) => a + rest.size
    case _ => "?"
  }

  def ixShape(v: IndexedSeq[Int]): Int = v match {
    case IndexedSeq(a, b, c) => a * b * c
    case _ => -1
  }

  def arrShape(v: Array[Int]): Int = v match {
    case Array(a, b) => a + b
    case Array(a, rest @ _*) => a + rest.size
    case _ => -1
  }

  def refArr(v: Array[String]): String = v match {
    case Array(a, rest @ _*) => a + rest.mkString("|")
    case _ => "?"
  }

  def nested(v: Seq[Seq[Int]]): Int = v match {
    case Seq(Seq(a, b), rest @ _*) => a + b + rest.size
    case _ => -1
  }

  def pairs(v: Seq[(String, Int)]): String = v match {
    case Seq((k, n), _*) => k + n
    case _ => "?"
  }

  def caseElems(ps: List[P]): Int = ps match {
    case List(P(x, y), rest @ _*) => x + y + rest.size
    case _ => 0
  }

  // A `_: T` sub-pattern is a test, not a cast: the element that fails it has
  // to fall through to the next case, not throw.
  def typedInList(xs: List[(String, Nd)]): String = xs match {
    case List((s, _: TableNd)) => "table " + s
    case List((s, _)) => "plain " + s
    case _ => "?"
  }

  def typedInSeq(v: Seq[(String, Nd)]): String = v match {
    case Seq((s, _: TableNd)) => "table " + s
    case Seq((s, _)) => "plain " + s
    case _ => "?"
  }

  def typedInOption(o: Option[(String, Nd)]): String = o match {
    case Some((s, _: TableNd)) => "table " + s
    case Some((s, _)) => "plain " + s
    case _ => "?"
  }

  // An `Any` scrutinee has to be *tested* first: the wrapper's extension
  // methods throw on anything that is not a sequence.
  def anyShape(x: Any): String = x match {
    case Array(a, b) => "arr " + a + b
    case Seq(a, b) => "seq " + a + b
    case List(a) => "lst " + a
    case _ => "?"
  }

  def main(args: Array[String]): Unit = {
    println(seqShape(Nil))
    println(seqShape(List(1)))
    println(seqShape(Vector(1, 2)))
    println(seqShape(Vector(1, 2, 3, 4)))
    println(listShape(List("x", "y", "z", "w")))
    println(listShape(List("q")))
    println(vecShape(Vector("a", "b")))
    println(vecShape(Vector("a", "b", "c")))
    println(ixShape(IndexedSeq(2, 3, 4)))
    println(arrShape(Array(1, 2)))
    println(arrShape(Array(1, 2, 3)))
    println(refArr(Array("x", "y", "z")))
    println(nested(Vector(Vector(1, 2), Vector(3))))
    println(pairs(Vector(("k", 7))))
    println(caseElems(List(P(1, 2), P(3, 4), P(5, 6))))
    // `"abc".map(_.toString)` is an `ArraySeq` at run time: a `Seq` pattern
    // has to read it by index, not by walking a cons list.
    val chars: Seq[String] = "abc".map(c => c.toString)
    println(chars match {
      case Seq(a, b, c) => a + b + c
      case _ => "?"
    })
    println(anyShape(Array(1, 2)))
    println(anyShape(Vector(1, 2)))
    println(anyShape(List(1, 2)))
    println(anyShape(List(9)))
    println(anyShape("x"))
    println(anyShape(5))
    println(typedInList(List(("a", new TableNd))))
    println(typedInList(List(("a", new Nd))))
    println(typedInSeq(Vector(("b", new TableNd))))
    println(typedInSeq(Vector(("b", new Nd))))
    println(typedInOption(Some(("c", new TableNd))))
    println(typedInOption(Some(("c", new Nd))))
  }
}
