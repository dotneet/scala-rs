package vc_generic_signature

trait Rep[A]
trait Query[A, B, C[_]]

final class QueryOps[B, P, C[_]](val query: Query[Rep[P], _, C]) extends AnyVal

object Conversions {
  implicit class PairOps[A, B](val pair: (A, B)) extends AnyVal

  implicit def queryOps[B, C[_]](
      query: Query[Rep[B], _, C]
  ): QueryOps[B, B, C] = new QueryOps[B, B, C](query)
}
