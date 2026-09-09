class Wrapped[A](val value:A) extends AnyVal
trait Transform[A] { def apply(a:A):A }
trait Source[A] { def get():A }
object Main {
 def main(args:Array[String]):Unit={
  val w:Transform[Wrapped[String]]=new Transform[Wrapped[String]] { def apply(a:Wrapped[String]):Wrapped[String]=new Wrapped(a.value+"!") }
  println(w(new Wrapped("ok")).value)
  val n:Transform[Wrapped[Int]]=new Transform[Wrapped[Int]] { def apply(a:Wrapped[Int]):Wrapped[Int]=new Wrapped(a.value+1) }
  println(n(new Wrapped(4)).value)
  val a:Transform[Wrapped[Any]]=new Transform[Wrapped[Any]] { def apply(x:Wrapped[Any]):Wrapped[Any]=new Wrapped(x.value) }
  println(a(new Wrapped[Any]("any")).value)
  val source:Source[Wrapped[Any]]=new Source[Wrapped[Any]] { def get():Wrapped[Any]=new Wrapped[Any]("source") }
  println(source.get().value)
 }
}
