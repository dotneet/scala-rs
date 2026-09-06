object Main {
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> 1, "b" -> 2)
    val selected = m.withFilter(_._2 > 0)
    val labels: Iterable[String] = selected.map { case (k, v) => k + v }
    val expanded: Iterable[String] = selected.flatMap { case (k, v) => List(k, k + v) }
    val pairs: Map[String, Int] = selected.map { case (k, v) => k -> (v + 1) }
    val flatPairs: Map[String, Int] = selected.flatMap { case (k, v) => List(k -> (v + 2)) }
    val render: ((String, Int)) => String = kv => kv._1 + kv._2
    println(labels.toList.sorted.mkString(","))
    println(expanded.toList.sorted.mkString(","))
    println(pairs.toList.sorted.mkString(","))
    println(flatPairs.toList.sorted.mkString(","))
    println(selected.map(render).toList.sorted.mkString(","))
  }
}
