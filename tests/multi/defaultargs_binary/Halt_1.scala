// scalatra's `Control.halt` shape, kept in its own file because it is
// compiled by real scalac only.
//
// Two overloads of one name, only one of which has defaults, and that one
// carries a context bound whose type parameter *no default getter's signature
// mentions*: nsc infers `halt$default$2`'s result from the default expression,
// so the getter is `[T]()Unit` and `T` is a leftover.
//
// scala-rs cannot compile this declaration itself yet -- it types the default
// `()` against the parameter's declared `T` and reports a mismatch, where nsc
// infers the getter's result type instead. That is a separate defect from the
// one this fixture is for, which is reading such a method back out of a class
// file; see `docs/default-arguments.md`.
package dalib

import scala.reflect.ClassTag

trait Control {
  def halt[T: ClassTag](status: Int = 400, body: T = (), reason: String = "why"): String =
    status + "|" + body + "|" + reason + "|" + implicitly[ClassTag[T]].runtimeClass.getSimpleName
  def halt(result: Boolean): String = "flag:" + result
}
