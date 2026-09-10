trait P {type T;val value:T}
trait Q extends P {type T=Array[Int];def read:Int=value(0)}
class N extends Q {lazy val value=Array(7)}
object Main {def main(args:Array[String]):Unit={val n=new N;println(n.read);println(n.read)}}
