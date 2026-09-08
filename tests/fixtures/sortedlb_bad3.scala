// The neighbouring case for `sortedExplicit` in `sortedlb.scala`: the same
// call *without* the type argument. `B` is pinned at the bound `A`, and the
// method's own `AA` witness does not answer for it.
trait Order[A]
class Chain[+A] {
  def sorted[AA >: A](implicit ord: Order[AA]): Chain[AA] = ???
}
class NEC[+A](val c: Chain[A]) {
  def ok[AA >: A](implicit o: Order[AA]): NEC[AA] = new NEC(c.sorted[AA])
  def bad[AA >: A](implicit o: Order[AA]): NEC[AA] = new NEC(c.sorted)
}
