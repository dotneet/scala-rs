class FSCell[T](var value:T) {lazy val cached:T=value}
trait FSProperty {type T;val value:T}
trait FSIntProperty extends FSProperty {type T=Int;def read:Int=value}
