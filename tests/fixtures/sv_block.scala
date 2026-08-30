// A block whose last statement is a *definition* has the value `()`
// (nsc `TreeBuilder.makeBlock`). Before this was modelled, the block took the
// definition's type, and every consumer that discards a block's value emitted a
// `pop` against a stack the definition never pushed:
//   java.lang.VerifyError: Operand stack underflow ... @2: pop
object Main {
  def valLast(): Unit = { println("valLast"); val v = 2 }
  def onlyVal(): Unit = { val v = 1 }
  def varLast(): Unit = { var n = 0 }
  def defLast(): Unit = { def f = 1 }
  def importLast(): Unit = { import scala.collection.immutable.Nil }
  def classLast(): Unit = { class C }
  def objectLast(): Unit = { object O }
  def typeLast(): Unit = { type T = Int }
  def emptyBlock(): Unit = {}
  def ifBranches(x: Int): Unit = { if (x > 0) { val y = 1 } else { val z = 2 } }
  def whileBody(x: Int): Unit = { var k = x; while (k > 0) { val y = k; k -= 1 } }
  def tryBody(): Unit = { try { val y = 1 } catch { case _: Throwable => () } }
  def matchBody(): Unit = { 1 match { case _ => val y = 1 } }
  def patternVal(): Unit = { val (p, q) = (1, 2) }
  def nestedBlock(): Unit = { println("nested"); { val a = 1 } }
  // A block that still ends in a term keeps that term's value.
  def valueBlock(): Int = { val v = 41; v + 1 }
  // The value of a definition-terminated block is `()`, so it may be ascribed.
  def unitValue(): Unit = { val u: Unit = { val v = 1 }; u }

  def lambdaBody(): Unit = {
    val f: Int => Unit = { q => val y = q }
    f(1)
    f(2)
  }

  def main(args: Array[String]): Unit = {
    valLast()
    onlyVal()
    varLast()
    defLast()
    importLast()
    classLast()
    objectLast()
    typeLast()
    emptyBlock()
    ifBranches(1)
    ifBranches(-1)
    whileBody(2)
    tryBody()
    matchBody()
    patternVal()
    nestedBlock()
    unitValue()
    lambdaBody()
    println(valueBlock())
    println("done")
  }
}
