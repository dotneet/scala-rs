class VSqlRsChild extends VSqlNscBase("jdbc:nsc")

object Main {
  def main(args: Array[String]): Unit = {
    val c = new VSqlRsChild
    println(c.url + ":" + c.user + ":" + c.password)
  }
}
