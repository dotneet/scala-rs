package cats.evidence

object PreludeUse {
  def use[A, B](ev: A <:< B): List[B] =
    ev.substituteCo[List](List.empty[A])
}
