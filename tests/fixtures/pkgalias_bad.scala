// A name the `scala` package object does not declare must still be an error:
// reading the pickle supplies the aliases that are there, not ones that are not.
object Main {
  def main(args: Array[String]): Unit = {
    val xs: NoSuchAliasHere[Int] = null
    println(xs)
  }
}
