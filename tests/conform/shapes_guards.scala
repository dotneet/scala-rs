object Main {
  sealed trait Shape
  final case class Circle(r: Double) extends Shape
  final case class Rect(w: Double, h: Double) extends Shape
  final case class Tri(a: Double, b: Double, c: Double) extends Shape
  def area(s: Shape): Double = s match {
    case Circle(r) => math.Pi * r * r
    case Rect(w, h) => w * h
    case Tri(a, b, c) => val p = (a+b+c)/2; math.sqrt(p*(p-a)*(p-b)*(p-c))
  }
  def describe(s: Shape): String = s match {
    case Circle(r) if r > 10 => "big circle"
    case _: Circle => "circle"
    case Rect(w, h) if w == h => "square"
    case r: Rect => f"rect ${r.w}%.1f"
    case Tri(_, _, _) => "tri"
  }
  def main(ar: Array[String]): Unit = {
    val ss = List(Circle(1), Rect(2,2), Rect(2,3), Tri(3,4,5), Circle(20))
    ss.foreach(s => println(f"${describe(s)}%-12s ${area(s)}%.3f"))
    println(ss.map(area).sum)
    println(ss.groupBy(describe).view.mapValues(_.size).toMap.toList.sorted)
  }
}
