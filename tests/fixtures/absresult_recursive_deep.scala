trait Pack[A, B] { def apply(a: A): B }
case class Cons[H, T](head: H, tail: T)
case object End
object Main {
  implicit val number: Pack[Int, String] = new Pack[Int, String] { def apply(a: Int) = a.toString }
  implicit val end: Pack[End.type, End.type] = new Pack[End.type, End.type] { def apply(a: End.type) = a }
  implicit def cons[A, T, B, U](implicit h: Pack[A, B], t: Pack[T, U]): Pack[Cons[A, T], Cons[B, U]] =
    new Pack[Cons[A, T], Cons[B, U]] { def apply(a: Cons[A, T]) = Cons(h(a.head), t(a.tail)) }
  def pack[A, B](a: A)(implicit p: Pack[A, B]): B = p(a)
  def main(args: Array[String]): Unit = println(pack(Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, End.type]]]]]]]]]]]](1, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, End.type]]]]]]]]]]](2, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, End.type]]]]]]]]]](3, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, End.type]]]]]]]]](4, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, End.type]]]]]]]](5, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, End.type]]]]]]](6, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, End.type]]]]]](7, Cons[Int, Cons[Int, Cons[Int, Cons[Int, Cons[Int, End.type]]]]](8, Cons[Int, Cons[Int, Cons[Int, Cons[Int, End.type]]]](9, Cons[Int, Cons[Int, Cons[Int, End.type]]](10, Cons[Int, Cons[Int, End.type]](11, Cons[Int, End.type](12, End))))))))))))))
}
