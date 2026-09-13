trait Witness[A]

object Witness {
  implicit val intWitness: Witness[Int] = new Witness[Int] {}
}
