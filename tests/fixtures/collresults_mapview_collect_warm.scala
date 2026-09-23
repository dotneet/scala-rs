object WarmMapCollect {
  val pairs = Map(1 -> 2).collect { case (key, value) => key -> value }
}

case class Answer(id: Int, at: Int, name: String)
case class IncorrectStudent(name: String)

class ViewCollect(logs: Seq[Answer]) {
  val incorrectStudents: Seq[IncorrectStudent] =
    logs.groupBy(_.id).view.mapValues(_.maxBy(_.at))
      .collect {
        case (_, latest) if latest.at < 0 => IncorrectStudent(latest.name)
      }.toSeq.sortBy(_.name)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(WarmMapCollect.pairs.size)
    println(new ViewCollect(Seq(Answer(1, -1, "Bob"))).incorrectStudents.map(_.name).mkString(","))
  }
}
