// A call that is genuinely ambiguous for SLS 6.26.3 has to stay ambiguous:
// neither alternative is as specific as the other (`B` is more specific than
// `Any` in the second parameter, `A` is more specific than `Any` in the
// first), and neither owner is a proper subclass of the other -- they are the
// same object. scalac 2.13.16 reports
//
//   ambiguous reference to overloaded definition, ... match argument types
//   (ovspec.B,ovspec.B)
//
// on the `m` line, and nothing on the two above it.
package ovspec

class A
class B extends A

object Bad {
  def m(x: Any, y: B): Int = 1
  def m(x: A, y: Any): Int = 2

  def main(args: Array[String]): Unit = println(m(new B, new B))
}
