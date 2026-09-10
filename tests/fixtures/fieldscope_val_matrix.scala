final class Meter(val n:Int) extends AnyVal
trait P {type T;val value:T}
trait QLong extends P {type T=Long;def read:Long=value}
trait QArray extends P {type T=Array[Int];def read:Int=value(0)}
trait QString extends P {type T=String;def read:Int=value.length}
trait QMeter extends P {type T=Meter;def read:Int=value.n}
trait QUnit extends P {type T=Unit;def read:Unit=value}
class NLong extends QLong {val value=9L}
class NArray extends QArray {val value=Array(7)}
class NString extends QString {val value="abc"}
class NMeter extends QMeter {val value=new Meter(8)}
class NUnit extends QUnit {val value=()}
object Main {def main(args:Array[String]):Unit={println(new NLong().read);println(new NArray().read);println(new NString().read);println(new NMeter().read);println(new NUnit().read)}}
