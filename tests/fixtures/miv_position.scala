object Main extends App {
  implicit val plain: MivPlain = new MivPlain {}
  println(Miv.positioned())
}
