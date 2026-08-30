object Main {
  def cmd(s: String): String = s match {
    case "get" => "GET"
    case "put" | "post" => "WRITE"
    case "del" => "DELETE"
    case other => s"?$other"
  }
  def code(n: Int): String = (n: @scala.annotation.switch) match {
    case 200 => "ok"; case 404 => "nf"; case 500 => "err"; case _ => "??"
  }
  def main(a: Array[String]): Unit = {
    println(List("get","put","post","del","x").map(cmd))
    println(List(200,404,500,302).map(code))
    val s: String = null
    println(cmd(if (s == null) "get" else s))
  }
}
