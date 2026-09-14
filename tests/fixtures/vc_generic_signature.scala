package vc_generic_signature

object Conversions {
  implicit class PairOps[A, B](val pair: (A, B)) extends AnyVal
}
