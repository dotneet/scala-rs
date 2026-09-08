// The two `Byte` boundary values, one step past the ones `negl_run.scala`
// accepts: `128` (positive, pre-existing plain-literal path) and `-129`
// (negated, this slice's path) must both still be rejected.
object NeglBadBoundary {
  val hi: Byte = 128
  val lo: Byte = -129
}
