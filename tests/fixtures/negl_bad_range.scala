// A negated literal is still bounded by the target's range: `-300` does not
// fit in a `Byte` any more than `300` would, so this must be rejected at the
// same line scalac rejects it at.
object NeglBadRange {
  val b: Byte = -300
}
