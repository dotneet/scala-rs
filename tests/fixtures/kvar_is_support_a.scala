package cats.evidence

private[evidence] trait IsSupport {
  def isFromPredef[A, B](implicit ev: A =:= B): Is[A, B] =
    ev.substituteCo[Is[A, *]](Is.refl[A])
}
