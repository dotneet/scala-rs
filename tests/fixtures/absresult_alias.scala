trait Root { type Out; def value: Out }
trait Mid[A] extends Root { type Out = A }
class Child[B](x: B) extends Mid[B] { def value = x }
object Main { def main(args: Array[String]): Unit = println(new Child[String]("alias").value) }
