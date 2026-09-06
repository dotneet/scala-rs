class VSqlNscChild extends VSqlRsBase("jdbc:rs")

object Main {
  def main(args: Array[String]): Unit = {
    val c = new VSqlNscChild
    println(c.url + ":" + c.user + ":" + c.password)
  }
}
