import p1.A
import p1.{C1, T1}
import p1.p2.*
import p1.p2.p3.{D => DD, C3 => Third}
import p1.p2.po.*
import p1.p2.po.Inner.h

object Main {
  def main(args: Array[String]): Unit = {
    println(A.f)
    println(new C1().g)
    println(new Helper().t)
    println(B.f)
    println(new C2().g)
    println(DD.f)
    println(new Third().g)
    println(pof)
    println(h)
  }
  class Helper extends T1 { def t: Int = 3 }
}
