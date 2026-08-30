// implicit def local to a method body used as a view: once for a plain
// conversion (assignment coercion), once as the source of an extension
// method on a locally declared (non-implicit) class.
object Main {
  def main(a: Array[String]): Unit = {
    implicit def i2s(n: Int): String = "n" + n
    val str: String = 5
    println(str)

    class G(val n: Int) { def trp = n * 3 }
    implicit def toG(n: Int): G = new G(n)
    println(3.trp)
  }
}
