// scalac: ambiguous reference to overloaded definition -- `who(Any)` defined
// in the subclass earns a point for its owner, `who(String)` one for being
// more specific, and the tie is ambiguous.
object Main {
  class Parent { def who(x: Any): String = "parent-any"; def who(x: String): String = "parent-string" }
  class Child extends Parent { override def who(x: Any): String = "child-any" }
  def main(args: Array[String]): Unit = println(new Child().who("s"))
}
