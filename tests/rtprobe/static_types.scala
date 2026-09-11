// Inferred static types, made observable through overload resolution: the
// overload `which` picks tells us what type the compiler assigned. A wrong
// inferred type (e.g. `Map.updated` answering at the wrong type) compiles
// cleanly and silently picks another overload.
import scala.collection.{immutable, mutable}
object Main {
  def which(x: immutable.Map[String, Int]): String = "imm.Map[String,Int]"
  def which(x: immutable.SortedMap[String, Int]): String = "SortedMap[String,Int]"
  def which(x: List[Int]): String = "List[Int]"
  def which(x: Vector[Int]): String = "Vector[Int]"
  def which(x: Seq[Int]): String = "Seq[Int]"
  def which(x: Set[Int]): String = "Set[Int]"
  def which(x: immutable.SortedSet[Int]): String = "SortedSet[Int]"
  def which(x: Int): String = "Int"
  def which(x: Long): String = "Long"
  def which(x: Double): String = "Double"
  def which(x: Float): String = "Float"
  def which(x: Char): String = "Char"
  def which(x: String): String = "String"
  def which(x: Option[Int]): String = "Option[Int]"
  def which(x: Some[Int]): String = "Some[Int]"
  def which(x: mutable.Map[String, Int]): String = "mutable.Map"
  def which(x: Iterable[Int]): String = "Iterable[Int]"
  def which(x: Any): String = "Any"
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> 1)
    val sm = immutable.SortedMap("a" -> 1)
    println(which(m.updated("b", 2)) + " " + which(m + ("c" -> 3)) + " " + which(m - "a") + " " + which(m.filter(_._2 > 0)))
    println(which(sm.updated("b", 2)) + " " + which(sm + ("c" -> 3)) + " " + which(sm.filter(_._2 > 0)) + " " + which(sm.map { case (k, v) => (k, v + 1) }))
    println(which(List(1).map(_ + 1)) + " " + which(Vector(1) :+ 2) + " " + which(List(1) ++ Vector(2)) + " " + which(Seq(1).map(_ * 2)))
    println(which(Set(1) + 2) + " " + which(immutable.SortedSet(1) + 2) + " " + which(immutable.SortedSet(3, 1).map(_ * 2)) + " " + which(Set(1).map(_.toString.length)))
    println(which(1 + 1) + " " + which(1 + 1L) + " " + which(1 + 1.0f) + " " + which(1L * 2.0) + " " + which('a' + 1) + " " + which('a') + " " + which(('a' + 1).toChar))
    val b: Byte = 1; val s: Short = 2
    println(which(b + b) + " " + which(s * 2) + " " + which(b.toLong) + " " + which(-b))
    println(which(if (true) 1 else 2L) + " " + which(if (true) 1 else 'c') + " " + which(if (true) Some(1) else None) + " " + which(Some(1)))
    println(which(Option(1).map(_ + 1)) + " " + which(List(1, 2).headOption) + " " + which(List(1, 2).sum) + " " + which(List(1.5).sum) + " " + which(List(1L).max))
    println(which(mutable.Map("a" -> 1).updated("b", 2)) + " " + which(mutable.Map("a" -> 1) += ("b" -> 2)))
    println(which(List(1, 2).view.map(_ + 1).toList) + " " + which((1 to 3).toList) + " " + which((1 to 3).map(_ + 1)) + " " + which(1 to 3))
    println(which("ab".length) + " " + which("ab" + 1) + " " + which("ab".head) + " " + which("ab".map(_.toUpper)) + " " + which("ab".map(_.toInt)))
    println(which(math.max(1, 2)) + " " + which(math.max(1, 2L)) + " " + which(math.abs(-1.5f)) + " " + which(1.0 / 2) + " " + which(7 / 2))
    println(which(Map(1 -> 2).map(_._2)) + " " + which(Map("x" -> 1).map { case (k, v) => (k, v) }) + " " + which(Map("x" -> 1).keySet.map(_.length)))
    val x = 3; val y: Long = x
    println(which(x) + " " + which(y) + " " + which(x.toFloat) + " " + which(x.toDouble) + " " + which(x: Any))
    println(which(List(1, 2) match { case h :: _ => h; case Nil => 0L }) + " " + which(try 1 catch { case _: Exception => 2.0 }))
  }
}
