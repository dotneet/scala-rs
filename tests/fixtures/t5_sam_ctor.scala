// A function literal against a SAM-shaped (not literal `scala.FunctionN`)
// constructor parameter of a callee with only one candidate -- a case
// class's synthesized `apply`, not a genuine overload.
//
// `Builder(sql, (u, pp) => ...)` against
// `case class Builder(sql: String, setParameter: SetParameter[Unit])`
// where `SetParameter[-T] extends ((T, PositionedParameters) => Unit)`
// left the literal's parameters entirely untyped
// (`(<notype>, <notype>) => <notype>`) at the scoring stage, because
// pre-typing a function literal against a callee's expected parameter
// shape (`agreed_lambda_params`, nsc's `pretypeArgs`) only ever ran for a
// genuine `Overload` of two or more alternatives -- and `arg_score` itself
// only recognized a literal `scala.FunctionN` parameter, not a trait that
// merely *extends* one. Both gaps together meant scoring `Builder(...)`
// against its one real candidate failed outright: "no matching overload
// for (String, SetParameter[Unit])Builder with arguments (String,
// (<notype>, <notype>) => <notype>)" -- slick's
// `SQLActionBuilder(sql, (u, pp) => ...)` against the identically-shaped
// `case class SQLActionBuilder(sql: String, setParameter:
// SetParameter[Unit])`.
//
// The fix is only in `arg_score`: a class-shaped parameter that is
// SAM-convertible (`SymbolTable::sam_sig`) is compared as the function
// type its abstract method describes, same as a literal `FunctionN`
// already was. `agreed_lambda_params`'s own pre-typing was deliberately
// *not* widened to a single-candidate callee -- tried and measured
// end-to-end against slick, it pre-typed a literal against a still-
// type-parametric single-candidate signature elsewhere (cats-effect's
// `Async[F].uncancelable[A](body: Poll[F] => F[A]): F[A]`) before the
// call's own inference had solved `A`, which regressed far more than this
// fixed. The literal still ends up correctly typed either way --
// `adapt_args_to_params`, run once the real (and here, only) candidate is
// known, retypes every argument against its actual parameter type.
object Main {
  trait PositionedParameters {
    var calls: Int = 0
    def touch(u: Unit): Unit = { calls += 1 }
  }
  trait SetParameter[-T] extends ((T, PositionedParameters) => Unit) {
    def apply(v1: T, v2: PositionedParameters): Unit
  }
  case class Builder(sql: String, setParameter: SetParameter[Unit])

  def make: Builder = Builder("x", (u, pp) => pp.touch(u))

  def main(args: Array[String]): Unit = {
    val b = make
    val pp = new PositionedParameters {}
    b.setParameter.apply((), pp)
    println(b.sql)
    println(pp.calls)
  }
}
