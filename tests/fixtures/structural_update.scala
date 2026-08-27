class Cell {
  private var n: Int = 0
  def foo: Int = n
  def foo_=(k: Int): Unit = { n = k }
}
class Buf {
  private var n: Int = 0
  def apply(i: Int): Int = n
  def update(i: Int, v: Int): Unit = { n = v }
}
object Main {
  def setVar(x: { var foo: Int }): Unit = {
    x.foo = 41
  }
  def getVar(x: { var foo: Int }): Int = x.foo
  def setDef(x: { def foo: Int; def foo_=(k: Int): Unit }): Unit = {
    x.foo = 7
  }
  def upd(x: { def apply(i: Int): Int; def update(i: Int, v: Int): Unit }): Unit = {
    x(0) = 9
  }
  def main(args: Array[String]): Unit = {
    val c = new Cell()
    setVar(c)
    println(getVar(c))
    setDef(c)
    println(getVar(c))
    val b = new Buf()
    upd(b)
    println(b.apply(0))
  }
}
