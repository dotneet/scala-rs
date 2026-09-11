// scala-rs rejects: eta-expanding the overloaded Predef.println against a
// function prototype `Int => B`.
object Main {
  def main(args: Array[String]): Unit = {
    println(List(1, 2, 3).map(println).length)
    List("a", "b").foreach(println)
  }
}
