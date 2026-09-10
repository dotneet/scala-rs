class Base(val prefix: String, values: Int*) {
  def answer: String = prefix + values.sum
}
class Child(n: Int) extends Base("sum=", n, n + 1)
object Empty extends Base("sum=")
object Main {
  def main(args: Array[String]): Unit = {
    println(new Child(4).answer)
    println(Empty.answer)
    println(new Base("sum=", 7, 8).answer)
  }
}

