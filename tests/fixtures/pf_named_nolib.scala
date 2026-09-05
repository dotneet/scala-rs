// The `--no-scala-library` half of the named-argument fix. The private
// runtime has no pickle to read the library's parameter names out of, so the
// compiler says so rather than guessing a name or accepting the call silently.
object Main {
  def main(args: Array[String]): Unit = {
    val o: Option[Int] = Some(1)
    println(o.getOrElse(default = 0))
  }
}
