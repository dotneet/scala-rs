// `Array` is invariant, and reading `Any` as `Object` for a Java method's type
// parameter does not change that: `copyOf[Any]` still wants an `Array[Object]`,
// so an `Array[String]` is rejected -- real scalac rejects it too
// ("found: Array[String] required: Array[Any]", its `Array[Any]` being the
// `Object` array).
object Main {
  def main(args: Array[String]): Unit = {
    val ss = new Array[String](2)
    val a = java.util.Arrays.copyOf[Any](ss, 3)
    println(a.length)
  }
}
