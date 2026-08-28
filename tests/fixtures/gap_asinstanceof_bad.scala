// `Type::Null` and an unbounded `Type::TypeParam` now resolve members through
// `Any`/`AnyRef` (asInstanceOf, eq, ...), but that must stay narrow: a method
// that genuinely does not exist on Any/AnyRef is still an error.
object Main {
  def main(args: Array[String]): Unit = {
    println(null.thisMethodDoesNotExist)
  }
}
