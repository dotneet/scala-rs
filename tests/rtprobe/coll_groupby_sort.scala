// groupBy, groupMap, sortBy (with tuple keys and reverse orderings),
// sortWith, sorted stability, partition, span, distinctBy, and maxBy/minBy.
object Main {
  case class Emp(name: String, dept: String, salary: Int, age: Int)
  def main(args: Array[String]): Unit = {
    val emps = List(Emp("ann", "eng", 120, 30), Emp("bob", "ops", 90, 45), Emp("cid", "eng", 100, 25), Emp("dee", "ops", 90, 35), Emp("eve", "hr", 70, 50))
    val byDept = emps.groupBy(_.dept)
    println(byDept.keys.toList.sorted)
    println(byDept.map { case (d, es) => d -> es.map(_.name) }.toList.sortBy(_._1))
    println(emps.groupMap(_.dept)(_.salary).view.mapValues(_.sum).toList.sortBy(_._1))
    println(emps.groupMapReduce(_.dept)(_ => 1)(_ + _).toList.sorted)
    println(emps.sortBy(_.salary).map(_.name))
    println(emps.sortBy(e => (-e.salary, e.name)).map(_.name))
    println(emps.sortBy(_.age)(Ordering[Int].reverse).map(_.name))
    println(emps.sortWith((a, b) => a.dept < b.dept || (a.dept == b.dept && a.age > b.age)).map(_.name))
    println(emps.maxBy(_.salary).name + " " + emps.minBy(_.age).name + " " + emps.map(_.salary).max)
    val (hi, lo) = emps.partition(_.salary >= 100)
    println(hi.map(_.name) + " " + lo.map(_.name))
    val (pre, post) = emps.span(_.dept != "hr")
    println(pre.length + " " + post.map(_.name))
    println(emps.distinctBy(_.dept).map(_.name) + " " + emps.map(_.salary).distinct)
    println(List(3, 1, 2).sorted + " " + List("b", "A", "a", "B").sorted + " " + List(2.5, -1.0, 0.0).sorted)
    println(List((1, "b"), (1, "a"), (0, "z")).sortBy(_._1))  // stable
    println(List("apple", "fig", "banana", "kiwi").sortBy(_.length) + " " + List("apple", "fig", "banana").sortBy(s => (s.length, s)))
    println(Vector(5, 3, 8).sorted(Ordering.Int.reverse) + " " + Array(3, 1, 2).sortBy(-_).toList)
    println(emps.count(_.dept == "eng") + " " + emps.exists(_.age > 49) + " " + emps.forall(_.salary > 60) + " " + emps.find(_.dept == "ops").map(_.name))
    println(emps.map(_.salary).sum / emps.size + " " + emps.foldLeft(0)(_ + _.age))
    println(emps.groupBy(_.salary > 95).view.mapValues(_.size).toMap.toList.sorted)
    println(List(1, 2, 3, 4, 5, 6).grouped(4).toList + " " + List(1, 2, 3, 4).sliding(2).toList + " " + List(1, 2, 3, 4, 5).sliding(3, 2).toList)
    println(emps.sortBy(_.name).reverse.take(2).map(_.name) + " " + emps.takeWhile(_.salary > 80).size + " " + emps.dropWhile(_.salary > 80).size)
    println(List(1, 2, 3, 4, 5).splitAt(2) + " " + List(1, 2, 3).tails.toList + " " + List(1, 2, 3).inits.toList.size)
  }
}
