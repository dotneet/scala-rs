// Control case: a local implicit val filling a nested method's implicit
// parameter already worked before this slice. Kept alongside lc_class /
// lc_conv / lc_shadow as the baseline the view-search fix has to match.
object Main {
  def main(a: Array[String]): Unit = {
    implicit val s: String = "iv"
    def g(implicit x: String) = x
    println(g)
  }
}
