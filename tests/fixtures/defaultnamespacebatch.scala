object Main {
  object Types { type Function[A,B] = Map[A,B] }
  object Scope {
    import Types._
    val m: Function[String,Int] = Map("a" -> 1)
    def result: String = Function.const[String,Int]("ok")(m("a"))
  }
  def main(args:Array[String]):Unit={
    println(Scope.result)
    import scala.collection.immutable._
    val im = IntMap(1 -> 2)
    println(im.map { case (k,v) => (k,v.toString) }.isInstanceOf[IntMap[_]])
    val lm = LongMap(1L -> 2)
    println(lm.map { case (k,v) => (k,v.toString) }.isInstanceOf[LongMap[_]])
  }
}
