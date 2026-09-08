// Explicit type arguments are the instantiation of an *overloaded* callee too
// (SLS 6.26.3; nsc's `Infer.inferPolyAlternatives`).
//
// Before, the written `[…]` reached the call only after an alternative had
// been picked, so every alternative was instantiated by inferring from the
// value arguments alone. Here that solves `T` to the least upper bound of
// `Integer` and `scala.Int`, and `Xbox` is invariant -- so the first
// alternative was rejected, the second wants a `String` for its first
// argument and was rejected too, and the call was `no matching overload`
// where scalac boxes the `4` and picks the first.
//
// This is gitbucket's `EditorConfigUtil.scala:129` with the jar spelled out:
// `props.getValue[Integer](PropertyType.tab_width, TabSizeDefault, false)`
// against ec4j's `(PropertyType[T], T, boolean)T` and `(String, T, boolean)T`.
// It is the one *stated* regression the `agent/triemapjava` slice left behind
// (gitbucket 270 -> 271), and reading a Java field's `Signature` correctly is
// what exposed it: `tab_width` only became a `PropertyType[Integer]` then.
//
// Which alternative runs is a run-time difference, not a compile-time one, so
// each prints its own name. Real scalac 2.13.16 prints the same three lines.
package ovsc

class Xbox[T](val t: T)

object Sel {
  def getValue[T](p: Xbox[T], d: T, b: Boolean): String =
    "xbox-alternative:" + p.t + ":" + d + ":" + b
  def getValue[T](s: String, d: T, b: Boolean): String =
    "string-alternative:" + s + ":" + d + ":" + b
}

object Main {
  def main(args: Array[String]): Unit = {
    // `T := Integer` written; the `4` is a `scala.Int` and is boxed.
    println(Sel.getValue[Integer](new Xbox[Integer](Integer.valueOf(7)), 4, false))
    // The other alternative, reached the same way.
    println(Sel.getValue[Integer]("s", 5, true))
    // The type argument still loses to nothing when it fits exactly: with an
    // `Integer` for the second parameter as well, the pre-fix compiler picked
    // this same alternative, because inference alone could reach it.
    println(Sel.getValue[Integer](new Xbox[Integer](Integer.valueOf(1)), Integer.valueOf(2), false))
  }
}
