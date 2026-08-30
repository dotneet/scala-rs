// Two methods of the same object may each declare a `trait Same` / `class SC`
// / `object O`. Their simple names are only unique inside one method, so nsc
// gives every local declaration a fresh index (`Main$Same$1`, `Main$Same$2`).
// Without one both classfiles were called `Main$Same` and the second silently
// overwrote the first: `dupA()` printed `dupB`.
object Main {
  def dupA(): String = {
    trait Same { def s = "A" }
    class SC extends Same { def both = s + "a" }
    object O { def g = "oa" }
    new SC().both + O.g
  }

  def dupB(): String = {
    trait Same { def s = "B" }
    class SC extends Same { def both = s + "b" }
    object O { def g = "ob" }
    new SC().both + O.g
  }

  def shadowedName(): Unit = {
    class P(val x: Int) { override def toString = "P" + x }
    println(new P(1))
    if (true) {
      class P(val x: Int) { override def toString = "Q" + x }
      println(new P(2))
    }
  }

  def main(args: Array[String]): Unit = {
    println(dupA())
    println(dupB())
    shadowedName()
  }
}
