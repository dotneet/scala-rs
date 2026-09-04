import ifb._

class Impl extends Narrow[Int]

object Main {
  def main(args: Array[String]): Unit = {
    val c = new Impl
    println(c.build)
    println((c: Ops[Int]).build)
    println((c: Ops[Int]).fac.name)
    println(c.toString)
    println(c.hashCode)
    println(c == new Impl)

    val a = new Narrow[Int] {}
    println((a: Ops[Int]).build)
    println((a: Ops[Int]).fac.name)
    println(a.toString)

    val w = new Wide[Int] {}
    println((w: Ops[Int]).fac.name)
    println(w.toString)
  }
}
