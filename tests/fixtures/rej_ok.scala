// Variance (SLS 4.5) and self-type conformance are the two rules whose whole
// job is to *reject*. Both rejected shapes scalac 2.13.16 accepts, and every
// case below was checked against real scalac 2.13.16 before being written down.
//
//   1-3. The variance of a type argument is read off the *type constructor's
//        own* parameters. nsc reads `sym.typeParams` whatever the head is; a
//        head that is an abstract type member or a higher-kinded type
//        parameter has declared variances just as a class does.
//   4-5. A self type is written in the declaring trait's vocabulary. Reading
//        it here means substituting the parent's type parameters *and*
//        resolving the abstract type members the enclosing cake has since
//        aliased.
object Main {
  // ---- 1. an abstract type member carries its own variances ---------------
  trait NoStream
  trait Streaming[+T] extends NoStream
  trait Effect
  trait BasicAction[+R, +S <: NoStream, -E <: Effect] {
    type ResultAction[+R, +S <: NoStream, -E <: Effect] <: BasicAction[R, S, E]
  }
  trait BasicStreamingAction[+R, +T, -E <: Effect] extends BasicAction[R, Streaming[T], E] {
    def head: ResultAction[T, NoStream, E]
    def headOption: ResultAction[Option[T], NoStream, E]
  }

  // ---- 2. the same, with the member's parameters named apart --------------
  trait SqlAction[+R, +S <: NoStream, -E <: Effect] extends BasicAction[R, S, E] {
    type ResultAction[+R0, +S0 <: NoStream, -E0 <: Effect] <: SqlAction[R0, S0, E0]
    def overrideStatements(s: String): ResultAction[R, S, E]
  }

  // ---- 3. a higher-kinded type parameter carries its variances too --------
  trait Box[F[+X], -G[-Y], +A] {
    def get: F[A]
    def put(g: G[A]): Unit
  }

  // ---- 4. a cake whose self type is an abstract type member the subtrait
  //         aliases; `Database[F]` means `JdbcDatabaseDef[F]` from here on ---
  trait BasicBackend {
    type Database[F[_]] >: Null <: BasicDatabaseDef[F]
    trait BasicDatabaseDef[F[_]] { this: Database[F] =>
      def tag: String
    }
  }
  trait JdbcBackend extends BasicBackend {
    type Database[F[_]] = JdbcDatabaseDef[F]
    abstract class JdbcDatabaseDef[F[_]] extends BasicDatabaseDef[F] {
      def tag = "jdbc"
    }
  }
  object JdbcBackend extends JdbcBackend {
    // The anonymous class has to conform to the same self type.
    def make[F[_]]: Database[F] = new JdbcDatabaseDef[F] {}
  }

  // ---- 5. a plain parameterized self type ---------------------------------
  trait Q[A] {
    def q: A
  }
  trait P[A] { self: Q[A] =>
    def p: A = q
  }
  class Ok(val q: Int) extends P[Int] with Q[Int]

  def main(args: Array[String]): Unit = {
    println(JdbcBackend.make[List].tag)
    println(new Ok(7).p)
  }
}
