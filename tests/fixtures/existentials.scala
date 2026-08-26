object Main {
  def show(xs: List[_]): Unit = {
    xs.foreach((x: Any) => println(x))
  }
  def show2(xs: List[X] forSome { type X }): Unit = {
    xs.foreach((x: Any) => println(x))
  }
  def main(args: Array[String]): Unit = {
    show(1 :: 2 :: Nil)
    show2("a" :: "b" :: Nil)
  }
}
