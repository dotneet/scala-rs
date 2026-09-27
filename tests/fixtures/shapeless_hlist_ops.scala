import shapeless._
import shapeless.ops.hlist.Reverse

// `take(2)` takes a `Nat`, supplied from the literal by the macro conversion
// `Nat.apply(i: Int) = macro NatMacros.materializeWidened`; `reverse` derives
// `Reverse0` one element at a time, the accumulator growing as the input
// shrinks.
object Main {
  type R = Int :: String :: Boolean :: HNil
  val r: R = 1 :: "a" :: true :: HNil
  def main(args: Array[String]): Unit = {
    println(r.take(2))
    println(r.reverse)
    println(r.reverse.head)
    val n: Nat = 2
    println(Nat.toInt[Nat._3])
    println(implicitly[Reverse[Int :: String :: Boolean :: Double :: HNil]].apply(1 :: "b" :: false :: 2.5 :: HNil))
    // Ten levels of derivation, past the depth at which a rule that repeats
    // without shrinking is cut off.
    val r8 = 1 :: "a" :: true :: 2.0 :: 3L :: 'c' :: Some(4) :: List("x") :: HNil
    println(r8.reverse.head)
  }
}
