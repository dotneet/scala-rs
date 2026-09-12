// `agent/kindvar`, round 1 of a separate compilation (`kvar_sep_use.scala`
// is compiled against this round's class files). The variance of these
// classes reaches the second round only through their pickles.
sealed trait \/[+A, +B]
sealed trait EitherT[F[+_], +A, +B]
class X[+A]
class Inv[A]
class Contra[-A]
