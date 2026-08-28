object Main {
  def main(args: Array[String]): Unit = {
    val xs = (1, "a") :: (2, "b") :: Nil
    for ((n, s) <- xs) println(n + s)
    val ys = for ((n, s) <- xs) yield s
    println(ys)
    val os: List[Option[Int]] = Some(1) :: None :: Some(3) :: Nil
    for (Some(v) <- os) println(v)
  }
}
