// Call sites for the implementation references of `mt2_mdef.scala`.
// `docs/macros.md` §7.22.
//
// Real scalac 2.13.16 compiles this file against the same class files and
// `crates/cli/tests/mapto2.rs` compares the two programs' output line for
// line. Every line here is a type argument that had to be *resolved* rather
// than lined up: a tag that took the call site's type argument where nsc takes
// the prefix's -- or the other way round -- would still compile and still run;
// only the output would differ.
import mt2._
import scala.language.experimental.macros

// The same shape declared in *this* run rather than read from a pickle: the
// macro def goes through `crates/typer/src/macros.rs` and the implementation
// through the class file. Both halves of the reader have to agree.
class LocalShaped[U](val u: U) {
  def mapTo[R]: String = macro Mt2Impl.pairImpl[R, U]
}

object Main {
  def main(args: Array[String]): Unit = {
    // `R` from the call site, `U` from the receiver.
    println(new Shaped[Int](3).mapTo[String])
    // The receiver's own argument is itself applied.
    println(new Shaped[List[String]](List("a")).mapTo[Double])
    // The owner's parameter reached through a subclass: `SubShaped[Char]`
    // has to be seen as `Shaped[Char]` first.
    println(new SubShaped[Char]('x').mapTo[Int])
    // Both written type arguments are the macro def's own, in the other
    // order.
    println(Plain.swapped[Int, String])
    // One from the call site, one written out in full.
    println(Plain.withFixed[Char])
    // The implementation's tag clause is in the other order from its type
    // parameters.
    println(Plain.flipped[Short])
    // The macro def compiled in this run.
    println(new LocalShaped[Long](1L).mapTo[Byte])
  }
}
