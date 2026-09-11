// scalac: return outside method definition.
object Main {
  val f: () => Int = () => return 1
  def main(args: Array[String]): Unit = println(f())
}
