object Main {
  class Carb[A]
  def bar[M[_], A]: Carb[M[A]] = new Carb[M[A]]
  val x: List[Int] = List(1)
  val bad: Carb[x.type] = bar
}
