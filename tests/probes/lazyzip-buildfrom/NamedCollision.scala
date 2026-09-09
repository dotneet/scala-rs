class Wrapped[A](val value:A) extends AnyVal
trait Transform[A] { def apply(a:A):A }
class Impl extends Transform[Wrapped[Any]] { def apply(x:Wrapped[Any]):Wrapped[Any]=new Wrapped(x.value) }
