import scala.language.implicitConversions
class Raw(val n: Int)
class Wrapped(val n: Int)
object Wrapped { implicit def wrap(raw: Raw): Wrapped = new Wrapped(raw.n + 1) }
abstract class Base { def value: Wrapped }
abstract class OverBase { def make(x: Any): Wrapped }
abstract class GenericBase[A] { def make(x: A): Wrapped }
abstract class PolyBase { def echo[A](x: A): A }
abstract class Broad { def value: Any }
abstract class Recursive { def loop(n: Int): String }
