trait P {type T;val value:T}
trait Q extends P {type T=Int;def read:String=value}
object Main {}
