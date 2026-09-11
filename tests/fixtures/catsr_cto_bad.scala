// `@compileTimeOnly`: every line scalac 2.13.16 rejects ends in `// error`,
// and no other line may be rejected.
import scala.annotation.compileTimeOnly

class C2
@compileTimeOnly("C2") object C2

object pkg {
  @compileTimeOnly("C5")
  implicit class C5(val x: Int) {
    def ext = ???
  }
}
class C6(@compileTimeOnly("C6.x") val x: Int)
@compileTimeOnly("C8") class C8[T]
@compileTimeOnly("placebo")
class placebo extends scala.annotation.StaticAnnotation
@compileTimeOnly("C3") case class C3(x: Int) // error

@compileTimeOnly("K") class K { def m = 1 }
object Uses {
  @compileTimeOnly("inner") def inner: K = new K
  @compileTimeOnly("wrap") def wrap = inner.m
  val c = classOf[K] // error
  def poly[T] = 0
  val p = poly[K] // error
  type Al = K // error
  def viaAlias(a: Al) = a
  def isK(x: Any) = x.isInstanceOf[K] // error
  def asK(x: Any) = x.asInstanceOf[K] // error
  def callInner = inner // error
  val nil = List[K]()
  val empty = List.empty[K] // error
}

class V(val s: String) extends AnyVal {
  @compileTimeOnly("error")
  def error = ???
}

object Test {
  C2 // error
  val a = C2 // error
  val c6 = new C6(2)
  val b = c6.x // error
  val c710: (C8[_] => C8[_]) = ??? // error
  import pkg._
  val e = 2.ext // error
  val k = new K // error
  val c3 = C3(1) // error
}
@placebo
class Test2 // error
