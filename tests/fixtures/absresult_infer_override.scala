trait Base {
  def method: Any
  val field: Any
  var mutable: Any
}
class Impl extends Base {
  def method = "method"
  val field = "field"
  var mutable = "initial"
}
class Constant extends Base {
  def method = "method"
  final val field = "constant"
  var mutable: Any = "initial"
}
object Main {
  def main(args: Array[String]): Unit = {
    val x = new Impl
    x.mutable = 42
    println(x.method)
    println(x.field)
    println(x.mutable)
    val constant: String = new Constant().field
    println(constant)
  }
}
