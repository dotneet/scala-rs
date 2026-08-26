object Main {
  def use(x: { def foo: Int }): Unit = {
    x.foo = 1
  }
}
