// Compiled by *real scalac* against the library above -- once against our class
// files and once against scalac's own -- and run. Every line here failed to
// compile against our pickles before `agent/rhfix`.
import rhf._
import rhfa._

object Main {
  def main(args: Array[String]): Unit = {
    // An inherited higher-kinded abstract type member, named through the object
    // and through another unit's alias, with the ops conversion found in the
    // object's implicit scope.
    val s: NSet[Int] = SetImpl.of(1, 2, 3)
    println(s.head)
    println(s.toList)
    println(s.size)
    println(SetImpl.of("a", "b").toList)

    // The same for a newtype that declares `Type` itself.
    val c: NChain[Int] = ChainImpl.of(4, 5)
    println(c.head)
    println(c.toList)
    println(ChainImpl.of(9).toList)

    // The `Aux` pattern: the refinement and the dependent result type.
    val r = Rep[String]
    println(r.tabulate(b => if (b) 1 else 0))
    println(Rep.fixed.tabulate(b => if (b) 2 else 3))

    // `Byte` and `Short` in a signature.
    println(Bytes.encode(Bytes.decode("hello")))
    println(Bytes.bump(1.toByte))
    println(Bytes.widen(2.toShort))
    println(new String(Bytes.decode("round")))
  }
}
