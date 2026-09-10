object Main {
  val c = new ResultCollection[String]("ok")
  val bad: Set[String] = c.values[Any]
}
