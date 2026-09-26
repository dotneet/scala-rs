// nsc `validateParentClasses`: a parent class or trait written twice is an
// error at every occurrence, through an alias and at any type arguments.
trait T0 { def a0: Int = 0 }
trait T1 extends T0
class C extends T0 with T1 with T0
class D extends T1 with T0 with T1 with T1
trait E extends T0 with T0
object O extends T1 with T1
class F { val x = new T0 with T0 }
class G extends AnyRef with T0 with AnyRef
abstract class R extends Runnable with Runnable
class H extends Seq[Int] with Seq[String]
object Al { type A0 = T0 }
class L extends T0 with Al.A0
