object Main {
  trait Monoid[A] { def empty: A; def combine(x: A, y: A): A }
  object Monoid {
    implicit val intM: Monoid[Int] = new Monoid[Int] { def empty = 0; def combine(x: Int, y: Int) = x + y }
    implicit val strM: Monoid[String] = new Monoid[String] { def empty = ""; def combine(x: String, y: String) = x + y }
    implicit def listM[A]: Monoid[List[A]] = new Monoid[List[A]] { def empty = Nil; def combine(x: List[A], y: List[A]) = x ::: y }
    implicit def tupleM[A, B](implicit ma: Monoid[A], mb: Monoid[B]): Monoid[(A, B)] =
      new Monoid[(A, B)] { def empty = (ma.empty, mb.empty); def combine(x: (A,B), y: (A,B)) = (ma.combine(x._1,y._1), mb.combine(x._2,y._2)) }
  }
  def fold[A](xs: List[A])(implicit m: Monoid[A]): A = xs.foldLeft(m.empty)(m.combine)
  def main(a: Array[String]): Unit = {
    println(fold(List(1,2,3)))
    println(fold(List("a","b")))
    println(fold(List(List(1), List(2,3))))
    println(fold(List((1,"a"), (2,"b"))))
    println(fold(List.empty[Int]))
  }
}
