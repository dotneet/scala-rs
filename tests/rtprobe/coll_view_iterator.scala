// Views and iterators are lazy: side effects happen only when forced, in
// element order; iterator exhaustion; LazyList memoization.
object Main {
  def main(args: Array[String]): Unit = {
    val log = new StringBuilder
    val v = (1 to 5).view.map { x => log.append(s"m$x;"); x * 2 }.filter { x => log.append(s"f$x;"); x > 4 }
    println("before force: [" + log + "]")
    println(v.take(2).toList)
    println("after: " + log)
    log.clear()
    val strict = (1 to 3).map { x => log.append(s"s$x;"); x }.filter { x => log.append(s"t$x;"); true }
    println(strict + " " + log)
    val it = List(1, 2, 3).iterator
    println(it.next() + " " + it.hasNext + " " + it.toList + " " + it.hasNext)
    val it2 = Iterator(1, 2, 3, 4, 5)
    val (a, b) = it2.span(_ < 3)
    println(a.toList + " " + b.toList)
    println(Iterator.from(1).map(_ * 3).filter(_ % 2 == 0).take(3).toList)
    println(Iterator.continually("x").take(3).mkString)
    val grouped = Iterator(1, 2, 3, 4, 5).grouped(2).toList
    println(grouped)
    var computed = 0
    lazy val ll: LazyList[Int] = LazyList.from(1).map { x => computed += 1; x * x }
    println(ll.take(3).toList + " computed=" + computed)
    println(ll.take(3).toList + " computed=" + computed)
    println(ll(4) + " computed=" + computed)
    lazy val fibs: LazyList[BigInt] = BigInt(0) #:: BigInt(1) #:: fibs.zip(fibs.tail).map { case (x, y) => x + y }
    println(fibs.take(12).toList)
    println(LazyList(1, 2, 3).map(_ + 1).force + " " + LazyList.empty[Int].headOption)
    val bi = Iterator(1, 2, 3).buffered
    println(bi.head + " " + bi.head + " " + bi.next() + " " + bi.head)
    val zipped = Iterator("a", "b").zip(Iterator.from(10)).toList
    println(zipped)
    val vw = Vector(1, 2, 3).view
    println(vw.map(_ * 10).reverse.toList + " " + vw.zipWithIndex.toList + " " + vw.slice(1, 2).toList + " " + vw.sum)
    println((1 to 1000000000).view.map(_ + 1).take(3).toList)
    val itSum = Iterator(1, 2, 3)
    println(itSum.sum + " " + itSum.isEmpty)
    val sliding = Iterator(1, 2, 3, 4).sliding(2).map(_.sum).toList
    println(sliding)
    println(Iterator.tabulate(4)(i => i * i).toList + " " + Iterator.range(0, 10, 4).toList + " " + Iterator.empty.hasNext)
    val it3 = List(5, 6, 7).iterator
    val dropped = it3.drop(1)
    println(dropped.next() + " " + it3.hasNext)
    println(List(1, 2, 3).view.flatMap(x => List(x, -x)).toList + " " + List(1, 2, 3).view.collect { case 2 => "two" }.toList)
  }
}
