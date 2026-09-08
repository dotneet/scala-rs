// `agent/libnotype`: what the three fixes must still reject.
package libnt

object Bad {
  def one(f: Int => Int): Int = f(1)

  // A blank line really does end the expression, so the block below is a
  // separate statement and `one` is a method reference with no argument list.
  def blankLineIsNotAnArgument: Int = {
    one

    { (x: Int) => x }
  }

  // `new` names a type; nothing here supplies one.
  def missing: Any = new NotAType

  // `new T` on a type parameter is still "class type required".
  def onTypeParam[T]: Any = new T
}
