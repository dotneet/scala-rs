import ctxreg.Ctx
import ctxreg.Out

object Main {
  def need()(implicit out: Out): String = out.text

  def wildcard(): String = {
    import ctxreg.Implicits._
    implicit val c: Ctx = new Ctx {}
    need()
  }

  def wildcardStar(): String = {
    import ctxreg.Implicits.*
    implicit val c: Ctx = new Ctx {}
    need()
  }

  def named(): String = {
    import ctxreg.Implicits.conv
    implicit val c: Ctx = new Ctx {}
    need()
  }

  def main(args: Array[String]): Unit =
    println(wildcard() + ":" + wildcardStar() + ":" + named())
}
