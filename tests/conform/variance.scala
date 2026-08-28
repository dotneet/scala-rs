class Box[+A](val xs: List[A]) {
  def add[B >: A](b: B): Box[B] = new Box(b :: xs)
  def head: A = xs.head
}
class Sink[-A](val f: A => Int) {
  def apply(a: A): Int = f(a)
}
object Main {
  def main(args: Array[String]): Unit = {
    val b: Box[Any] = new Box(List("a", "b")).add(1)
    println(b.xs)
    val s: Sink[String] = new Sink[Any](x => x.toString.length)
    println(s("hello"))
    val o: Option[Any] = Some(3)
    println(o)
  }
}
