// The cast the fix inserts must not be reached by silently accepting a wrong
// type: a factory result still has to be checked against what the context
// wants, and `:::` still only takes a `List`.
object Main {
  def main(args: Array[String]): Unit = {
    val a: Vector[Int] = List.fill(2)(5)
    val b: List[String] = List.fill(2)(5)
    println(List.fill(2)(5) ::: Vector(9))
    println(a, b)
  }
}
