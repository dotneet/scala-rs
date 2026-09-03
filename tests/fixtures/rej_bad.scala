// The reject side of `rej_ok.scala`: honouring the declared variances of a
// type member / higher-kinded parameter must not turn the variance rule off,
// and computing a self type properly must not stop it from failing.
// scalac 2.13.16 rejects every declaration below. In one run it prints only
// the two `illegal inheritance` errors, because those come out of the typer
// and nsc never reaches refchecks (where variance is validated) after a typer
// error; the four variance errors were read off scalac from a file holding
// just those four traits, and are quoted at each one.
object Main {
  // A type member with no variance annotations is invariant, so a covariant
  // parameter may not be handed to it.
  //   covariant type A occurs in invariant position in type Inv.this.M[A] of method f
  trait Inv[+A] {
    type M[X]
    def f: M[A]
  }

  // A `-X` parameter flips the position: a covariant `A` lands contravariantly.
  //   covariant type A occurs in contravariant position in type Flip.this.N[A] of method f
  trait Flip[+A] {
    type N[-X]
    def f: N[A]
  }

  // The same two, with a higher-kinded *type parameter* as the head.
  //   covariant type A occurs in invariant position in type F[A] of method f
  trait HkInv[F[X], +A] {
    def f: F[A]
  }
  //   covariant type A occurs in contravariant position in type G[A] of method f
  trait HkFlip[G[-Y], +A] {
    def f: G[A]
  }

  // A parameterized self type is still a self type.
  //   illegal inheritance; self-type Main.Miss[A] does not conform to
  //   Main.P[A]'s selftype Main.P[A] with Main.Q[A]
  trait Q[A]
  trait P[A] { self: Q[A] => }
  class Miss[A] extends P[A]

  // And one reached through a cake's type alias: `Database[F]` is
  // `Real[F]` here, which `Fake` is not.
  //   illegal inheritance; self-type Cake.this.Fake[F] does not conform to
  //   Cake.this.DbDef[F]'s selftype Cake.this.Database[F]
  trait Backend {
    type Database[F[_]] >: Null <: DbDef[F]
    trait DbDef[F[_]] { this: Database[F] => }
  }
  trait Cake extends Backend {
    type Database[F[_]] = Real[F]
    class Real[F[_]] extends DbDef[F]
    class Fake[F[_]] extends DbDef[F]
  }

  def main(args: Array[String]): Unit = println("unreachable")
}
