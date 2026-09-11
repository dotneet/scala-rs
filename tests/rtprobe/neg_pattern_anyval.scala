// scalac: type AnyVal cannot be used in a type pattern or isInstanceOf test.
object Main {
  def main(args: Array[String]): Unit = println((1: Any) match { case _: AnyVal => "anyval"; case _ => "ref" })
}
