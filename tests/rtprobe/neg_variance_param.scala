// scalac: covariant type A occurs in contravariant position in type A of
// value a.
object Main {
  class Box[+A](val v: A) { def put(a: A): Box[A] = new Box(a) }
  def main(args: Array[String]): Unit = println(new Box(1).put(2).v)
}
