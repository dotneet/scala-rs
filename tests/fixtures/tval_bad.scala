// A trait's `val` is not assignable: the mixin setter that backs it exists
// only for `T$class.$init$`, so nsc rejects the assignment outright.

trait Named {
  val label: String = "named"
}

class Plain extends Named

object Main {
  def main(args: Array[String]): Unit = {
    val p = new Plain
    p.label = "other"
    println(p.label)
  }
}
