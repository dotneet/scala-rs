class VSqlGenericGood extends VSqlGenericDefault[Int]()
class VSqlNumericChild extends VSqlNumericBase()
class VSqlCurriedChild extends VSqlCurriedBase("curried")()
class VSqlNestedChild extends VSqlNested.Base[Int]()
class VSqlAuxDefaultChild extends VSqlAuxDefaultBase()

object Main {
  def main(args: Array[String]): Unit = {
    println(new VSqlGenericGood().value)
    println(new VSqlNumericChild().value)
    println(new VSqlCurriedChild().prefix + ":" + new VSqlCurriedChild().value)
    println(new VSqlNestedChild().value)
    println(new VSqlAuxDefaultChild().value)
  }
}
