class VSqlUserDollarOuterChild extends VSqlUserDollarOuterBase(7)

object Main {
  def main(args: Array[String]): Unit = {
    val value = new VSqlUserDollarOuterChild
    println(value.x + ":" + value.`$outer`)
  }
}
