object Main {
  def main(args: Array[String]): Unit = {
    println(Miv.pathDependent())
    println(Miv.ordinary())
    locally {
      implicit val flag: MivPlain = new MivPlain {}
      println(Miv.ordinary())
    }
    val traced = Miv.traced()
    println(if (traced == null) "typed" else "unexpected")
  }
}
