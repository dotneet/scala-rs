// User definitions that reuse the names of standard types and members:
// a class named Int, an object named List, a local `println`, a type
// parameter named String. Each must resolve to the user's definition.
object Main {
  object Shadows {
    class Int(val v: scala.Int) { override def toString = "MyInt(" + v + ")" }
    object Int extends Int(7) { def apply(v: scala.Int): Int = new Int(v) }
    object List { def apply(xs: scala.Int*): java.lang.String = "MyList" + xs.mkString("<", ",", ">") }
    case class Option(tag: java.lang.String)
    def some = "shadowed some"
  }
  def generic[String](s: String): String = s
  def main(args: Array[String]): Unit = {
    import Shadows._
    println(Int); println(Int(3)); println(Int.v + 1); println(Int.getClass.getName)
    println(List(1, 2, 3)); println(scala.List(1, 2))
    println(Option("x")); println(scala.Option(1))
    println(some)
    println(generic(42))
    def println(x: Any): Unit = Predef.println("local println: " + x)
    println("hi")
    locally { val Predef = "not predef"; scala.Predef.println(Predef) }
    val max = 3
    scala.Predef.println(scala.math.max(max, 1))
    object Nil { override def toString = "MyNil" }
    scala.Predef.println(Nil + " " + scala.Nil)
    type Seq = scala.collection.immutable.List[scala.Int]
    val sq: Seq = scala.List(1)
    scala.Predef.println(sq.getClass.getSimpleName)
  }
}
