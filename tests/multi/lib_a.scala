package demo.util

class Box(val x: Int) {
  def doubled: Int = x * 2
}

object Box {
  def make(x: Int): Box = new Box(x)
}
