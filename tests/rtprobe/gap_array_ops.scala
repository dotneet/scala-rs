// scala-rs rejects: ArrayOps.indexOf (elem, from = 0) and sameElements
// on primitive arrays.
object Main {
  def main(args: Array[String]): Unit = {
    println(Array(1, 2, 3).indexOf(2) + " " + Array("a", "b").indexOf("b") + " " + Array(1, 2, 3).indexOf(3, 1))
    println(Array(1, 2, 3).sameElements(Array(1, 2, 3)) + " " + (Array(1) sameElements Array(2)) + " " + Array(1.5).sameElements(List(1.5)))
  }
}
