// agent/cats: cats' `Representable#compose`, written without kind-projector.
//
// The anonymous class defines `type Representation = (self.Representation,
// G.Representation)` while the trait it extends declares `Representation`
// abstract. `Type::TypeMember` carries no prefix, so expanding the right-hand
// side looked the name up again, found the anonymous class's own alias, and
// recursed until the 512MB stack ran out -- 244 cats-core sources produced no
// diagnostics at all, only `fatal runtime error: stack overflow`.
//
// Real scalac 2.13.16 accepts this file. We do not yet: the two prefixes still
// collapse onto the same member and the last line is reported as a type
// mismatch. What this fixture pins is that the compiler *terminates and says
// something*; `crates/cli/tests/cats4.rs` checks scalac's verdict alongside.

trait FunD[F[_]] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
  def compose[G[_]](implicit G: FunD[G]): FunD[({ type L[a] = F[G[a]] })#L] = ???
}

trait RepD[F[_]] { self =>
  def F: FunD[F]
  type Representation
  def tabulate[A](f: Representation => A): F[A]

  def compose[G[_]](implicit G: RepD[G]): RepD[({ type L[a] = F[G[a]] })#L] =
    new RepD[({ type L[a] = F[G[a]] })#L] {
      override val F = self.F.compose(G.F)
      type Representation = (self.Representation, G.Representation)
      def tabulate[A](f: Representation => A): F[G[A]] = {
        val fc: self.Representation => (G.Representation => A) = ???
        self.F.map(self.tabulate(fc))(G.tabulate(_))
      }
    }
}
