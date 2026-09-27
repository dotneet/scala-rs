// Names the alias `boxes.Box` first, then selects the *object*
// `boxes.Box` in term position. Once the alias had been read, the
// selection stopped at it and took `List`'s instance `map` and `one`.
object Client {
  def twice(b: boxes.Box[Int]): boxes.Box[Int] = boxes.Box.map(b)(x => x * 2)
  val made: boxes.Box[String] = boxes.Box.one("a")
  def main(args: Array[String]): Unit = println(List(twice(boxes.Box.one(21)), made))
}
