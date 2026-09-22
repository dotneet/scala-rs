object Main {
  val inferred = MethodInfo.inferred[Subject]
  def main(args:Array[String]):Unit = println(inferred)
}
class Subject {
  private def hidden = helper
  private def helper = 42
}
