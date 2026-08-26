object Main {
  def show(xs: List[_ <: AnyRef]): Unit = {
    xs.foreach((x: Any) => println(x))
  }
  def main(args: Array[String]): Unit = {
    show("a" :: "b" :: Nil)
  }
}
