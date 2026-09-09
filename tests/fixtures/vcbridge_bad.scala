class Meter(val value:Int) extends AnyVal
class Label(val value:String) extends AnyVal
trait Transform[A] { def apply(a:A):A }
object Bad {
 val m:Transform[Meter]=new Transform[Meter] { def apply(a:Meter):Meter=a }
 val wrongArgument = m(new Label("bad"))
 val wrongResult: Label = m(new Meter(1))
}
