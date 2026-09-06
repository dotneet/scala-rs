object VarianceExternalBad {
  val badSource: VarianceSource[VarianceDog] = new VarianceAnimalSource
  val badSink: VarianceSink[VarianceAnimal] = new VarianceDogSink
  val badBox: VarianceBox[VarianceDog] = new VarianceAnimalBox
}
