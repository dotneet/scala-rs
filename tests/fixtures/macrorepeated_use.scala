trait RepeatedApi { def accept(values: Int*): Int }
object Main {
  def main(args: Array[String]): Unit = println(RepeatedMirror.count[RepeatedApi])
}
