package testkitlib

object Main {
  import Prof._

  def main(args: Array[String]): Unit =
    println(greeting + "," + twice(21) + "," + api.col("abcd"))
}
