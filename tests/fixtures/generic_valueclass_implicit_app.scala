import genericvalueclass.Auto._

object Main {
  def main(args: Array[String]): Unit =
    println(implicitly[genericvalueclass.Reader[String]].read)
}
