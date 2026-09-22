object Bad {
  val wrong = implicitly[Evidence[Int] { type Out = Int }]
}
