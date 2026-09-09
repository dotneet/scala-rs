import multi.O.{Alias => OA}
import multi.P.{Alias => PA}
class C extends OA[String]("one")
class D extends PA[String]("two")
object Main { def main(args: Array[String]): Unit = { println(new C().n); println(new D().n); println(new C().value) } }
