object Main {
  object Types { type Function[A,B] = Map[A,B] }
  object Scope {
    import Types._
    val m: Function[String,Int] = Map("a" -> 1)
    def result: String = Function.const[String,Int]("ok")(m("a"))
  }
  def main(args:Array[String]):Unit=println(Scope.result)
}
