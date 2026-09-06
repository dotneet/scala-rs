trait ScopeBase {
  def foo: String = "base"
}

trait QualifiedLayer extends ScopeBase {
  def qualified: String = super[ScopeBase].foo
}

trait NestedLayer extends ScopeBase {
  class Inner extends ScopeBase {
    def helper: String = super.foo
  }

  def nested: String = new Inner().helper
}

class QualifiedBoth extends QualifiedLayer
class NestedBoth extends NestedLayer

object ScopeMain {
  def main(args: Array[String]): Unit = {
    println(new QualifiedBoth().qualified)
    println(new NestedBoth().nested)
  }
}
