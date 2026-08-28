object Main {
  def f(xs: List[Int]): String = xs match {
    case Nil => "nil"
    case h :: Nil => "one:" + h
    case h :: t :: Nil => "two:" + h + "," + t
    case h :: _ => "many:" + h
  }
  def opt(o: Option[Int]): Int = o match {
    case Some(x) => x * 2
    case None => -1
  }
  def main(args: Array[String]): Unit = {
    println(f(Nil))
    println(f(1 :: Nil))
    println(f(1 :: 2 :: Nil))
    println(f(1 :: 2 :: 3 :: Nil))
    println(opt(Some(21)))
    println(opt(None))
  }
}
