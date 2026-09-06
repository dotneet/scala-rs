// The two answers implicit search must still be able to give once it is
// memoized (`ImplicitMemo` in `crates/typer/src/implicits.rs`).
//
//  * `loop` derives itself, so the expansion has to be cut off by nsc's
//    `openImplicits` rule rather than recursed into. That rule reads mutable
//    state the memo key does not carry, which is why an entry is only stored
//    and reused where the open stack cannot have decided it.
//  * `t1` and `t2` are equally specific, so the answer is an ambiguity -- a
//    third result besides "found" and "not found", and one the memo has to
//    carry through unchanged.
//
// Real scalac 2.13.16 rejects both.
object Main {
  trait Box[A]
  trait Tag[A]

  implicit def loop[A](implicit a: Box[A]): Box[A] = a

  implicit val t1: Tag[Int] = new Tag[Int] {}
  implicit val t2: Tag[Int] = new Tag[Int] {}

  def needBox[A](implicit b: Box[A]): Unit = ()
  def needTag[A](implicit t: Tag[A]): Unit = ()

  def main(args: Array[String]): Unit = {
    needBox[Int]
    needTag[Int]
  }
}
