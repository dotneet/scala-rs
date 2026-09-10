class Base(val values: AnyRef*)
object Other
object Main extends Base(null, Other, null) {
  def main(args: Array[String]): Unit = {
    println(values.length)
    println(values(1) eq Other)
  }
}
