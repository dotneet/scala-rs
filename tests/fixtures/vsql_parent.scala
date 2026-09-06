// A parent call can select a primary constructor with trailing defaults even
// when the parent also declares an auxiliary constructor.
class VSqlBase(val url: String, val user: String = "user", val password: String = "password") {
  def this() = this("default", "user", "password")
}

class VSqlChild extends VSqlBase("jdbc:test")

object Main {
  def main(args: Array[String]): Unit = {
    val c = new VSqlChild
    println(c.url + ":" + c.user + ":" + c.password)
  }
}
