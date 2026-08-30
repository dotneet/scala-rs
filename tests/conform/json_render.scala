object Main {
  trait Json; case class JStr(s: String) extends Json; case class JNum(n: Double) extends Json
  case class JArr(xs: List[Json]) extends Json; case class JObj(fs: Map[String, Json]) extends Json
  case object JNull extends Json
  def render(j: Json): String = j match {
    case JStr(s) => "\"" + s + "\""
    case JNum(n) => if (n == n.toLong) n.toLong.toString else n.toString
    case JArr(xs) => xs.map(render).mkString("[", ",", "]")
    case JObj(fs) => fs.toList.sortBy(_._1).map { case (k, v) => "\"" + k + "\":" + render(v) }.mkString("{", ",", "}")
    case JNull => "null"
  }
  def main(a: Array[String]): Unit = {
    println(render(JObj(Map("b" -> JArr(List(JNum(1), JStr("x"), JNull)), "a" -> JNum(2.5)))))
  }
}
