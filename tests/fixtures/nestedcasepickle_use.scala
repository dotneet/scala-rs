import nestedcasepickle.Outer

object Main {
  def main(args: Array[String]): Unit = {
    val status = Outer.Status(
      url = None,
      enforcement_level = "everyone",
      contexts = Seq("ci"),
      contexts_url = None
    )
    println(status.enforcement_level + ":" + status.contexts.size)
  }
}
