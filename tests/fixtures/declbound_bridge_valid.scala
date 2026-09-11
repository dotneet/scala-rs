class Meter(val n:Int) extends AnyVal
class Text(val s:String) extends AnyVal
trait Base[T]{val value:T}
class M(val value:Meter) extends Base[Meter]
class S extends Base[Text]{lazy val value:Text=new Text("ok")}
object Main{def main(args:Array[String]):Unit={val m:Base[Meter]=new M(new Meter(7));val s:Base[Text]=new S;println(m.value.n);println(s.value.s)}}
