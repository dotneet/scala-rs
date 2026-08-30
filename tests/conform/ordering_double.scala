// `Ordering[Double]` / `Ordering[Float]` / `Ordering[Unit]`.
//
// 2.13 では `Ordering.Double` と `Ordering.Float` は名前空間オブジェクトに
// なっていて（`TotalOrdering` と `IeeeOrdering` を抱える）、implicit として
// 選ばれるのは `DeprecatedDoubleOrdering` / `DeprecatedFloatOrdering`。
object Main {
  trait Shape { def area: Double; def label: String }
  class Circle(r: Double) extends Shape { def area = 3.0 * r * r; def label = "C" }
  class Sq(s: Double) extends Shape { def area = s * s; def label = "S" }

  def main(args: Array[String]): Unit = {
    val ss: List[Shape] = List(new Sq(2.0), new Circle(1.0))
    println(ss.sortBy(_.area).map(_.label))
    println(List(3.5, 1.25, 2.0).sorted)
    println(List(3.5f, 1.25f, 2.0f).max)
    println(List(2.0, 1.0).min)
    println(implicitly[Ordering[Double]].compare(1.0, 2.0))
    println(implicitly[Ordering[Float]].compare(2.0f, 1.0f))
    println(implicitly[Ordering[Unit]].compare((), ()))
  }
}
