// `B => _` hides `B` from the trailing wildcard, so `B` stays unresolved
// even though `C2`, imported by the same wildcard, is fine.
import p1.p2.{B => _, _}

object Main {
  def main(args: Array[String]): Unit = {
    println(new C2().g)
    println(B.f)
  }
}
