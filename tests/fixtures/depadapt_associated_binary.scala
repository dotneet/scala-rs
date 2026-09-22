object Main {
  def main(args: Array[String]): Unit = {
    println(AssociatedValues.bind(new ValueHolder[NumberShape], 42))
    println(AssociatedValues.bind(new ValueHolder[TextShape], "ok"))
    println(AssociatedValues.element(new ValueHolder[NumberShape]))
  }
}
