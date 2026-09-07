// The negative half of `hkp_member.scala`: giving a higher-kinded type member
// a prefix must not make every prefix's member the same type, and must not
// invent a member the prefix does not have.
//
// Each of the three is rejected by real scalac 2.13.16 as well; the e2e test
// pins the lines and the types, not just the words.

case class Box[A](a: A)
case class One[A](a: A)

trait Par[M[_]] {
  type F[_]
  def par[A](ma: M[A]): F[A]
  def seq[A](fa: F[A]): M[A]
}

object Bad {
  // 1. Two different instances' `F` are two different type constructors.
  def mix[M[_], N[_]](p: Par[M], q: Par[N])(fa: p.F[Int]): q.F[Int] = fa

  // 2. And in the other direction.
  def mixBack[M[_], N[_]](p: Par[M], q: Par[N])(fa: q.F[Int]): p.F[Int] = fa

  // 3. A name the prefix's class does not declare is still not a member.
  def absent[M[_]](p: Par[M])(fa: p.G[Int]): Int = 0
}
