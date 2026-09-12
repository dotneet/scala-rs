// Every way a mirrored static member *is* reachable, against a `-cp` library
// real scalac compiled (`gz2_statlib.scala`): on the companion, through an
// `import`, and -- for a real Java class -- qualified and imported. The one way
// it is not is `gz2_statuse_bad.scala`.
import gz2lib.Gz2Base

class Gz2Sub extends Gz2Base(1) {
  def viaCompanion: String = Gz2Base.mk(1) + Gz2Base.tag
  def viaImport: String = { import Gz2Base._; mk(2) + tag }
  def inherited: String = own
}

object Gz2StatUse {
  def main(args: Array[String]): Unit = {
    val s = new Gz2Sub
    println(s.viaCompanion)
    println(s.viaImport)
    println(s.inherited)
    println(Integer.parseInt("41") + 1)
    System.out.println("sysout")
    locally {
      import java.lang.Integer._
      println(parseInt("7"))
      println(MAX_VALUE)
    }
  }
}
