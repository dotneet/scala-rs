// The Scala 3 wildcard spelling `.*` needs `-Xsource:3`; in plain 2.13 `*`
// is an ordinary name, so nothing is imported and `A` stays unresolved.
import p1.*

object Main {
  def main(args: Array[String]): Unit = println(A.f)
}
