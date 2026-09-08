// The two restrictions on the rule in `pkgobjdup_pkgobject.scala`, both of
// them shapes real scalac 2.13.16 rejects.
//
//   * `outer.Payload` is the package object's `val`, *not* the package's
//     `object Payload`, so the object's `tag` is not a member of it. nsc:
//     "value tag is not a member of object inner.Payload". A rule that merely
//     picked whichever alternative fits the selection would accept this.
//
//   * A package object whose `val` names the package member it is displacing
//     names *itself*. nsc: "recursive value Loop needs type" -- which is the
//     evidence that the package object's entry is the only `loopy.Loop` there
//     is, rather than a second binding beside the object's.
//
// The pre-fix binary reports the second of these as `value tag is not a member
// of <overload Payload$ | Payload$>`: the same diagnostic through the
// unreduced two-element set, which is the defect and not the rule.

package inner {
  object Payload { override def toString = "inner.Payload" }
}

package outer {
  object Payload { def tag: String = "outer's own object Payload" }
}

package object outer {
  val Payload = inner.Payload
}

package loopy {
  object Loop { def tag: String = "loopy's own object Loop" }
}

package object loopy {
  val Loop = loopy.Loop
}

object Main {
  def main(args: Array[String]): Unit = {
    println(outer.Payload.tag)
  }
}
