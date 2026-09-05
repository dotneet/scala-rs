// `@specialized` is a performance annotation: it must not change what type
// checks. `value` is a `T`, so assigning it to an `Int` is the same error with
// the annotation as without it.
class Box[@specialized(Int) T](val value: T) {
  val n: Int = value
}

object Main {
  def main(args: Array[String]): Unit = println(new Box(1).n)
}
