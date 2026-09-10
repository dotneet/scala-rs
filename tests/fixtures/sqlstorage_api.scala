class Hidden(private[this] val x: Int) { def get: Int = x }
class Plain(x: Int) { def get: Int = x }
class Private(private val x: Int) { def get: Int = x }
class Default(val x: Int = 1)
class Lazy { lazy val x: Int = 1 }
abstract class Abstract { val x: Int }
class Mutable(private[this] var x: Int) { def get: Int = x; def set(v: Int): Unit = { x = v } }
class HiddenBody { private[this] val x: Int = 9; def get: Int = x }
class HiddenLazy { private[this] lazy val x: Int = 8; def get: Int = x }
trait HiddenTrait { private[this] val x: Int = 7; def get: Int = x }
class ConcreteTrait extends HiddenTrait
class FinalParam(final val x: Int)
class ProtectedParam(protected val x: Int) { def get: Int = x }
class GenericDefault[A](val xs: List[A] = Nil)
object DefaultsOwner { class Nested(val x: Int = 11) }
class SecondaryDefault(val x: Int) { def this(s: String = "abcd") = this(s.length) }
object DefaultSource { var calls: Int = 0; def next(): Int = { calls += 1; calls } }
class EffectDefault(val x: Int = DefaultSource.next())
class CompanionDefault(val x: Int = 13)
object CompanionDefault { def label: String = "explicit" }
class PrivateOverload(private val x: Int) { def x(s: String): String = s }
