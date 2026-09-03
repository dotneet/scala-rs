// A reporting pipeline: groupBy -> mapValues -> toSeq -> sortBy, foldLeft with
// mutable state, and a `view` in the middle.
object Main {
  case class Row(dept: String, name: String, amount: Int, month: Int)

  val rows = List(
    Row("eng", "ann", 120, 1), Row("eng", "bob", 80, 1), Row("ops", "cid", 50, 1),
    Row("eng", "ann", 30, 2), Row("ops", "cid", 200, 2), Row("hr", "dee", 10, 2),
    Row("hr", "dee", 15, 3), Row("eng", "bob", 45, 3)
  )

  def main(args: Array[String]): Unit = {
    val byDept = rows.groupBy(_.dept).mapValues(_.map(_.amount).sum).toSeq.sortBy(-_._2)
    println(byDept.mkString("[", ", ", "]"))

    val byDeptThenName =
      rows.groupBy(r => (r.dept, r.name)).map { case (k, v) => k -> v.size }.toList.sorted
    println(byDeptThenName)

    var running = 0
    val cum = rows.sortBy(r => (r.month, r.name)).map { r => running += r.amount; (r.name, running) }
    println(cum)
    println(running)

    val total = rows.foldLeft(Map.empty[String, Int]) { (acc, r) =>
      acc.updated(r.dept, acc.getOrElse(r.dept, 0) + r.amount)
    }
    println(total.toSeq.sortBy(_._1))

    val v = rows.view.filter(_.amount > 40).map(r => r.name.toUpperCase).take(3).toList
    println(v)

    val grouped = rows.map(_.amount).grouped(3).toList
    println(grouped)
    println(rows.map(_.amount).sliding(2).map { case Seq(a, b) => b - a }.toList)

    val part = rows.partition(_.month == 1)
    println(part._1.size + "/" + part._2.size)
    println(rows.map(_.dept).distinct.sorted)
    println(rows.minBy(_.amount).name + " " + rows.maxBy(_.amount).name)
    println(rows.map(_.amount).scanLeft(0)(_ + _))
    println(rows.iterator.map(_.amount).filter(_ % 2 == 0).sum)
    println(rows.groupBy(_.month).toSeq.sortBy(_._1).map { case (m, rs) => s"$m:${rs.size}" }.mkString(","))
  }
}
