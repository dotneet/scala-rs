import scala.language.implicitConversions
class Raw(val n: Int)
class Wrapped(val n: Int)
object Wrapped { implicit def wrap(raw: Raw): Wrapped = new Wrapped(raw.n + 1) }
abstract class Base { def value: Wrapped }
class Child extends Base { def value = new Raw(7) }
object Main { def main(args: Array[String]): Unit = { val base: Base = new Child; println(base.value.n) } }
