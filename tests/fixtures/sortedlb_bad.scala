// An argument that forces a `B` violating `B >: A` is still rejected.
// `Ordering.String` gives `B = String`, which is not a supertype of `Int`;
// nsc reports the join it settled on instead, `Ordering[Any]`.
object Main {
  val xs: List[Int] = List(3, 1, 2)
  val a = xs.sorted(Ordering.String)
  val b = xs.max(Ordering.String)
}
