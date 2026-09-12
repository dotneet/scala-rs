// Constructor calls that compiled to a constructor the class does not have,
// and block-local classes whose class files overwrote each other.
import scala.collection.immutable.{TreeMap, TreeSet}
import scala.collection.mutable

class A { override def toString = "A" }
class B
// Both constructors are applicable to `new Foo`; the one needing a default
// loses to the repeated one (run/t8197).
class Foo(val x: A = null) {
  def this(bla: B*) = this(new A)
}
class G(val x: A = new A)
class H(val xs: Int*) { override def toString = "H" + xs.length }
class Z(val a: Int = 1) { def this(s: String) = this(s.length) }

// A context bound on a class whose constructor takes an `Array[T]`.
class BO[T: Ordering](val a: Array[T]) { def o = implicitly[Ordering[T]] }
class BI[T](val a: Array[T])(implicit val o: Ordering[T])
object Blarg { def apply[T: Manifest](a: Array[T]) = new Blarg(a) }
class Blarg[T: Manifest](val a: Array[T]) {
  def m[W >: T, S](f: W => S) = f(a(0))
}

object Test {
  // Block-local classes in field initialisers are local classes too.
  val a = { class L { def bar = 5 }; new L }
  val b = { class L { def bar = 6 }; new L }
  def m = { class L { def bar = 7 }; new L }
  val e = { class K; class L extends K; classOf[L] }
  class In {
    val a = { class L { def bar = 9 }; new L }
    val b = { class L { def bar = 10 }; new L }
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val tm = new TreeMap[Int, String]
    println(tm.updated(2, "b").updated(1, "a"))
    val tm2 = new TreeMap[Int, String]()
    println(tm2 + (3 -> "c"))
    val ts = new TreeSet[String]
    println(ts + "z" + "a")
    val mtm = new mutable.TreeMap[Int, Int]
    mtm(2) = 3; mtm(1) = 4
    println(mtm)

    println((new Foo).x + " " + (new Foo()).x)
    println((new G).x + " " + (new G()).x)
    println(new H + " " + new H() + " " + new H(1, 2))
    println((new Z).a + " " + new Z("abc").a)

    println(new BO(Array(3)).o.compare(1, 2))
    println(new BI(Array(3)).o.compare(2, 1))
    println(Blarg(Array(1, 2, 3)).m((x: Int) => x + 1))

    import scala.language.reflectiveCalls
    println(Test.a.bar + " " + Test.b.bar + " " + Test.m.bar)
    val in = new Test.In
    println(in.a.bar + " " + in.b.bar)
    println(Test.a.getClass.getName + " " + Test.b.getClass.getName)
    println(Test.e.getName)
  }
}
