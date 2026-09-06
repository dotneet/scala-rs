object VarianceExternalGood {
  def main(args: Array[String]): Unit = {
    val sourceDog: VarianceSource[VarianceDog] = new VarianceDogSource
    val sourceAnimal: VarianceSource[VarianceAnimal] = sourceDog
    val sinkAnimal: VarianceSink[VarianceAnimal] = new VarianceAnimalSink
    val sinkDog: VarianceSink[VarianceDog] = sinkAnimal
    println(sourceAnimal.get.name + ":" + sinkDog.put(new VarianceDog))
  }
}

class VarianceListWiden extends VarianceHKWiden[List] {
  def widen(xs: List[VarianceDog]): List[VarianceAnimal] = xs
}

class VarianceMethodImpl extends VarianceMethods {
  def identity[A](a: A): A = a
}
