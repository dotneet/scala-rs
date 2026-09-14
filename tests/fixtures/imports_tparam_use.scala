object Main {
  def target[A, B, C](a: A, f: A => Either[B, C]): String = "ok"

  def caller[A, B, C](a: A, f: A => Either[B, C]): String = {
    import imported.Exports._
    target[A, B, C](a, f) + ImportsTypeParamPreload.loaded._1
  }

  def main(args: Array[String]): Unit =
    println(caller[Int, String, Boolean](1, x => Left(x.toString)))
}
