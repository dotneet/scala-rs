// A qualified parent name blames whichever segment is missing: nsc reports
// the *value* when the prefix itself is unknown (SLS 3.2.3 -- the prefix of a
// type `p.T` is a term), the missing package segment when an intermediate one
// is, and the member otherwise. Every one of these compiled silently.
package pcq

object Holder { class Inner }
object Ob

class Q1 extends Holder.NoSuch
class Q2 extends pcq.NoSuchInPkg
class Q3 extends java.util.NoSuchJU
class Q4 extends Ob.Nope
class Q5 extends pkgless.Missing
class Q6 extends scala.collection.nosuchpkg.Foo

object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}
