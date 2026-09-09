class Meter(val value: Int) extends AnyVal
class Label(val value: String) extends AnyVal
trait Transform[A] { def apply(a:A):A }
object Main {
 def main(args:Array[String]):Unit={
  val m:Transform[Meter]=new Transform[Meter] { def apply(a:Meter):Meter=new Meter(a.value+1) }
  val l:Transform[Label]=new Transform[Label] { def apply(a:Label):Label=new Label(a.value+"!") }
  println(m(new Meter(4)).value)
  println(l(new Label("ok")).value)
  val make: String => Label = s => new Label(s)
  val read: Label => String = l => l.value
  println(read(make("function")))
 }
}
