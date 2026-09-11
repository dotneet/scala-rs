// foldLeft/foldRight/reduce/scan (and their evaluation order), zip family,
// flatMap/flatten, collect, and aggregate-style operations.
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3, 4)
    println(xs.foldLeft("")((acc, x) => s"($acc+$x)") + " " + xs.foldRight("")((x, acc) => s"($x+$acc)"))
    println(xs.reduce(_ - _) + " " + xs.reduceRight(_ - _) + " " + xs.reduceLeftOption(_ * _) + " " + List.empty[Int].reduceOption(_ + _))
    println(xs.scanLeft(0)(_ + _) + " " + xs.scanRight(0)(_ + _) + " " + xs.scan(1)(_ * _))
    println(xs.fold(10)(_ + _) + " " + xs.sum + " " + xs.product + " " + List(1.5, 2.5).sum + " " + List(2L, 3L).product)
    println(xs.zip(xs.tail) + " " + xs.zipWithIndex + " " + xs.zipAll(List("a"), 0, "z") + " " + (xs lazyZip xs.map(_ * 2)).map(_ + _))
    println(List(List(1, 2), Nil, List(3)).flatten + " " + xs.flatMap(x => List.fill(x % 3)(x)) + " " + List(Some(1), None, Some(3)).flatten)
    println(xs.collect { case x if x % 2 == 0 => x * 10 } + " " + xs.collectFirst { case x if x > 2 => "found " + x })
    val order = new StringBuilder
    xs.foldLeft(0) { (a, x) => order.append(x); a + x }
    xs.foldRight(0) { (x, a) => order.append(x); a + x }
    println(order)
    println(xs.map(_ * 2).filter(_ > 2).take(2) + " " + xs.filterNot(_ == 2) + " " + xs.withFilter(_ > 1).map(_ + 1))
    println(xs.head + " " + xs.last + " " + xs.init + " " + xs.tail + " " + xs.headOption + " " + Nil.headOption + " " + xs.lastOption)
    println(xs.indexOf(3) + " " + xs.indexWhere(_ > 1) + " " + xs.lastIndexOf(4) + " " + xs.contains(5) + " " + xs.isDefinedAt(3) + " " + xs.lift(10))
    println(xs.updated(1, 20) + " " + xs.patch(1, List(7, 8), 2) + " " + xs.padTo(6, 0) + " " + xs.intersect(List(2, 4, 6)) + " " + xs.diff(List(1)) + " " + (xs ++ List(1, 2)).distinct)
    println(xs.mkString + " " + xs.mkString("-") + " " + xs.mkString("<", ",", ">") + " " + xs.reverse + " " + xs.reverseIterator.toList)
    println(xs.partition(_ % 2 == 0) + " " + xs.groupBy(_ % 2).toList.sortBy(_._1) + " " + xs.corresponds(List(2, 4, 6, 8))(_ * 2 == _))
    println(xs.sameElements(Vector(1, 2, 3, 4)) + " " + xs.startsWith(List(1, 2)) + " " + xs.endsWith(List(4)) + " " + xs.containsSlice(List(2, 3)))
    println(xs.min + " " + xs.max + " " + xs.minOption + " " + List.empty[Int].maxOption + " " + xs.count(_ > 1) + " " + xs.length + " " + xs.size)
    println((1 to 5).toList.map(_.toDouble).sum / 5 + " " + List("a", "bb", "ccc").map(_.length).sum)
    println(xs.iterator.map(_ + 1).toList + " " + xs.unzip(x => (x, x * x)) + " " + xs.map(x => (x, x.toString)).unzip3(t => (t._1, t._2, t._1 * 2)))
    println(List.tabulate(3)(_ * 2) + " " + List.fill(2)("x") + " " + List.range(1, 10, 4) + " " + List.iterate(1, 4)(_ * 3) + " " + List.concat(List(1), List(2)))
    println(xs.foldLeft(List.empty[Int])((acc, x) => x :: acc) + " " + xs.foldRight(List.empty[Int])(_ :: _))
    println(xs.aggregate(0)(_ + _, _ + _))
  }
}
