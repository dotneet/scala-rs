// Imports: renaming, hiding with `=> _`, imports inside blocks shadowing
// outer ones, importing members of a value, and relative package imports.
object Main {
  object A { val x = "A.x"; def f(i: Int) = "A.f" + i; val y = "A.y" }
  object B { val x = "B.x"; def f(i: Int) = "B.f" + i }
  class Cfg(val host: String, val port: Int)
  def main(args: Array[String]): Unit = {
    import scala.collection.mutable.{Map => MMap, Set => _}
    val mm = MMap(1 -> 2); mm(3) = 4
    println(mm.toList.sorted + " " + Set(1).getClass.getName.contains("immutable"))
    import A.{x => ax, _}
    println(ax + " " + f(1) + " " + y)
    locally {
      import B._
      println(x + " " + f(2))
    }
    println(f(3))
    val cfg = new Cfg("localhost", 8080)
    import cfg._
    println(host + ":" + port)
    import java.util.{List => JList}
    val jl: JList[String] = java.util.Arrays.asList("j")
    println(jl.get(0) + " " + List(1).head)
    import scala.math.{max => biggest, Pi}
    println(biggest(3, 4) + " " + (Pi > 3))
    import scala.util.chaining._
    println(5.pipe(_ * 2) + " " + 5.tap(v => println("tapped " + v)))
  }
}
