package xmetabounds

// Keep all three bound shapes in one tiny provider so the interoperability
// test checks their pickles through the same classpath boundary.
class Pair[A <: B, B](val a: A, val b: B)

trait FBound[A <: Comparable[A]] {
  def value: A
}

trait LowerBound[A >: String] {
  def value: A
}
