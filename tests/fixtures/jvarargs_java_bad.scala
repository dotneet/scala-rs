// The same two ties, declared in Java and read back from a class file. Real
// scalac 2.13.16 reports `ambiguous reference to overloaded definition` at
// both calls -- lines 8 and 9.
package jvarargs

object JBad {
  def main(args: Array[String]): Unit = {
    println(JAmbig.b(1))
    println(JAmbig.d(1))
  }
}
