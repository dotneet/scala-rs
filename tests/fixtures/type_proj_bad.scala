trait Foo { type A }
class Bar extends Foo {
  type A = Int
  def x: A = 1
}
object Main {
  def bad(c: Bar): c.A = c.x
}
