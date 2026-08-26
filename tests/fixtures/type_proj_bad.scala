trait Foo { type A }
class Bar extends Foo {
  type A = Int
}
object Main {
  var v: Bar = new Bar()
  def bad: v.A = 1
}
