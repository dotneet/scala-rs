// A class parameter that is a field (a `val`, a `var`, any parameter of a
// case class's first list) cannot be by-name (nsc's parser). An implicit
// by-name parameter of a method is allowed.
object Test {
  case class ByName(y: => Int)
  class Field(val z: => Int)
  def imp(implicit w: => Int) = w
}
