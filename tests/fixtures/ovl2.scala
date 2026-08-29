// Overload candidate sets: what inheritance adds, what it hides, and the
// interfaces `java.lang.String` really implements.

// 1. Inheriting is not overriding. `C` adds an alternative to `B.f`; both are
//    members of `C`, so `f(1)` and `f("ab")` each resolve.
class Base { def f(x: Int): String = "int:" + x }
class Derived extends Base { def f(s: String): String = "str:" + s }
class Deeper extends Derived { def both: String = f(7) + "/" + f("z") }

// 2. A bare constructor parameter is `private[this]`, so it is not inherited:
//    `Sub`'s own `tag` is the only one its body sees.
class Outer(tag: String) { def outerTag: String = tag }
class Sub(tag: Int) extends Outer("outer") { def subTag: Int = tag + 1 }

// 3. A `val` that implements an abstract `def` is one member, not an overload.
trait HasName { def label: String }
class Named extends HasName {
  val label: String = "named"
  def shout: String = label + "!"
}

// 4. `java.lang.String` implements `CharSequence`, so a `String` is accepted
//    where the JDK asks for one, and `indexOf` has an `(Int)Int` alternative a
//    `Char` argument widens into.
object Main {
  def firstChar(cs: CharSequence): Char = cs.charAt(0)

  def main(args: Array[String]): Unit = {
    println(new Deeper().both)
    println(new Sub(41).subTag)
    println(new Sub(41).outerTag)
    println(new Named().shout)

    val s = "a:b:c"
    println(s.indexOf(':'))
    println(s.indexOf(":"))
    println(s.indexOf(':', 2))
    println(s.lastIndexOf(':'))
    println(firstChar(s))
    val cs: CharSequence = s
    println(cs.length)
  }
}
