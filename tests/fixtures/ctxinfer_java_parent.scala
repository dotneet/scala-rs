class Child(n: Int) extends ctxinfer_JavaParent("class=", n, n + 1)
object Values extends ctxinfer_JavaParent("object=", 4, 5)
object Empty extends ctxinfer_JavaParent("empty=")
object Spread extends ctxinfer_JavaParent("spread=", Array(6, 7): _*)
object Main {
  def main(args: Array[String]): Unit = {
    println(new Child(1).answer())
    println(Values.answer())
    println(Empty.answer())
    println(Spread.answer())
  }
}
