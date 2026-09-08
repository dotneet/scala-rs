// Every line here is one real scalac 2.13.16 rejects, and the point of the
// file is that the fix must not make any of them compile.
//
// Line 16 is the soundness half of root 2: with the mixin forwarder's
// flattened parameter list, `tagIn` looked like *one* clause of two, so
// passing the witness positionally alongside the argument was accepted.
// scalac says "too many arguments"; so do we.
import gboptlib.Entry._
import gboptlib.Box

object Bad {
  // `T` comes from the argument, so this asks for `Box[Option[Int]]` and
  // there is no `Box[Int]` to lift.
  def named[T](x: Option[T])(implicit b: Box[Option[T]]): String = b.name

  def a: String = named(Option(1))
  def b: String = 7.tagIn(List("x")).text
  def c: String = 7.tagIn(List(1), gboptlib.Wit.intWit).text
  def d: gboptlib.Res[Int] = 7.tagIn(List(1))
}
