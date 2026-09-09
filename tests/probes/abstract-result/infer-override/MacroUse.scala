trait Base { def method: Any; val field: Any }
class Use extends Base { def method = M.text; val field = M.text }
object Main {
  def main(args: Array[String]): Unit = {
    val x = new Use
    println(x.method)
    println(x.field)
  }
}
