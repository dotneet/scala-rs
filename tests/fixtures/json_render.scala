import scala.collection.mutable

sealed trait Json
case object JNull extends Json
case class JBool(b: Boolean) extends Json
case class JNum(n: Double) extends Json
case class JStr(s: String) extends Json
case class JArr(items: List[Json]) extends Json
case class JObj(fields: List[(String, Json)]) extends Json

object Render {
  def render(j: Json): String = j match {
    case JNull => "null"
    case JBool(b) => b.toString
    case JNum(n) => n.toString
    case JStr(s) => "\"" + s + "\""
    case JArr(items) => items.map(render).mkString("[", ",", "]")
    case JObj(fs) => fs.map { case (k, v) => "\"" + k + "\":" + render(v) }.mkString("{", ",", "}")
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val doc = JObj(List(
      ("name", JStr("scala-rs")),
      ("version", JNum(1)),
      ("tags", JArr(List(JStr("a"), JStr("b")))),
      ("ok", JBool(true)),
      ("none", JNull)
    ))
    println(Render.render(doc))
    val counts = mutable.Map[String, Int]()
    for (t <- List("a", "b", "a")) counts(t) = counts.getOrElse(t, 0) + 1
    println(counts.toList.sortBy(_._1))
  }
}
