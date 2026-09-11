// scalac: covariant type A occurs in contravariant position in type A of
// value a_= (a var of covariant type).
object Main {
  class Cell[+A](init: A) { var a: A = init }
  def main(args: Array[String]): Unit = println(new Cell(1).a)
}
