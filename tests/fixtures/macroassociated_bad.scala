object BadAssociated {
  val wrong = implicitly[Associated[Int] { type Out = Int }]
}
