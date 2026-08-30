// A local implicit def shadows a same-named outer one (SLS 7.2: candidates
// are identifiers reachable by ordinary unqualified name resolution, which
// shadows -- not two candidates that then have to be told apart).
object Main {
  implicit def i2s(n: Int): String = "outer" + n
  def main(a: Array[String]): Unit = {
    implicit def i2s(n: Int): String = "inner" + n
    val str: String = 5
    println(str)
  }
}
