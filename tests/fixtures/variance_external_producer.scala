class VarianceAnimal {
  def name: String = "animal"
}

class VarianceDog extends VarianceAnimal {
  override def name: String = "dog"
}

trait VarianceSource[+A] {
  def get: A
}

trait VarianceSink[-A] {
  def put(a: A): String
}

trait VarianceBox[A] {
  def get: A
  def put(a: A): String
}

trait VarianceMethods {
  def identity[A](a: A): A
}

trait VarianceHKWiden[F[+X]] {
  def widen(xs: F[VarianceDog]): F[VarianceAnimal]
}

class VarianceDogSource extends VarianceSource[VarianceDog] {
  def get: VarianceDog = new VarianceDog
}

class VarianceAnimalSource extends VarianceSource[VarianceAnimal] {
  def get: VarianceAnimal = new VarianceAnimal
}

class VarianceAnimalSink extends VarianceSink[VarianceAnimal] {
  def put(a: VarianceAnimal): String = a.name
}

class VarianceDogSink extends VarianceSink[VarianceDog] {
  def put(a: VarianceDog): String = a.name
}

class VarianceAnimalBox extends VarianceBox[VarianceAnimal] {
  def get: VarianceAnimal = new VarianceAnimal
  def put(a: VarianceAnimal): String = a.name
}
