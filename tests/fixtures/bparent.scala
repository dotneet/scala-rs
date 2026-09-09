import bparent.O.{Alias => OA}
import bparent.P.{Alias => PA}
class C extends OA[String]("one")
class D extends PA[String]("two")
class Q extends bparent.O.Alias[Int](3)
object W { import bparent.P._; class E extends Alias[Int](5) }
class E0 extends bparent.O.EmptyAlias
class S extends bparent.Static.Base(13)
import bparent.Holder.other._
class Forwarded extends Alias()
object Direct { import bparent.ApiP.api._; class C extends Alias() }
object ValueClient { import bparent.ValueO.api._; class C extends Alias() }
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().n)
    println(new D().n)
    println(new C().value)
    println(new Q().n)
    println(new W.E().n)
    println(new S().n)
    println(new E0().n)
    println(new Forwarded().n)
    println(new Direct.C().n)
    println(new ValueClient.C().n)
  }
}
