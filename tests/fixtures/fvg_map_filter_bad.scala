object Main {
  // The lambda returns a List, but WithFilter builds Iterable[String].
  val wrong: List[String] = Map("a" -> 1).withFilter(_._2 > 0)
    .flatMap { case (k, v) => List(k) }
}
