object Holder {
  def nestedScope = {
    class C { private def x = 0 }

    {
      val a = 0
      object C {
        new C().x
      }
    }
  }

  def deeperStill = {
    class D { private val y = 1 }
    if (true) {
      object D { def read = new D().y }
      D.read
    } else 0
  }
}
