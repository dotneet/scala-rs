object Main {
  var events = ""

  def mark(i: Int, x: Int): Int = {
    if (i >= 21) events += i.toString + ":" + x.toString + ";"
    x
  }

  def main(args: Array[String]): Unit = {
    val result = for {
      x <- List(1, 2)
      v1 = mark(1, x)
      v2 = mark(2, x)
      v3 = mark(3, x)
      v4 = mark(4, x)
      v5 = mark(5, x)
      v6 = mark(6, x)
      v7 = mark(7, x)
      v8 = mark(8, x)
      v9 = mark(9, x)
      v10 = mark(10, x)
      v11 = mark(11, x)
      v12 = mark(12, x)
      v13 = mark(13, x)
      v14 = mark(14, x)
      v15 = mark(15, x)
      v16 = mark(16, x)
      v17 = mark(17, x)
      v18 = mark(18, x)
      v19 = mark(19, x)
      v20 = mark(20, x)
      v21 = mark(21, x)
      v22 = mark(22, x)
    } yield v22
    println(result)
    println(events)
  }
}
