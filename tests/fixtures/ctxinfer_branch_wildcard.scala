object Main {
  case class Row(group: String, label: String, grade: String, name: String)
  val rows = Seq(Row("a", "s", "2", "second"), Row("a", "s", "1", "first"))
  val grades = Map("a" -> Seq("1", "2"))
  val groups = Map("a" -> 0)
  def main(args: Array[String]): Unit = {
    val sorted = rows.sortBy { row =>
      val gradeOrder = grades.getOrElse(row.group, Seq.empty)
      val index = gradeOrder.indexOf(row.grade)
      (groups.getOrElse(row.group, Int.MaxValue), row.label,
        if (index >= 0) index else Int.MaxValue, row.name)
    }
    val matched = rows.sortBy { row =>
      val index = grades(row.group).indexOf(row.grade)
      (row.group, index match { case -1 => Int.MaxValue; case n => n })
    }
    val strings = rows.sortBy(row => (row.group, if (row.name == "first") "a" else "b"))
    println(sorted.map(_.name).mkString(","))
    println(matched.map(_.name).mkString(","))
    println(strings.map(_.name).mkString(","))
  }
}
