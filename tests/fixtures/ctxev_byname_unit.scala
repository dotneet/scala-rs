object Main {
  var n = 0
  def discard(value: => Unit): Unit = value
  def ignore(value: => Unit): Unit = ()
  def lazyId[A](value: => A): A = value
  def foo[B](f: (=> Int) => B): () => B = () => f(0)
  def overloaded(value: => Unit): String = "unit"
  def overloaded(value: () => Unit)(implicit dummy: DummyImplicit): String = "function"
  def main(args: Array[String]): Unit = {
    discard(() => { n += 1 })
    println(n)
    ignore(() => { n += 1 })
    println(n)
    discard({ n += 1; () => { n += 100 } })
    println(n)
    println(foo(value => value)())
    println(foo(lazyId)())
    println(overloaded(() => ()))
  }
}
