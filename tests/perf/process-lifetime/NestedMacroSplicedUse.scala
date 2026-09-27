package shapeprobe

object NestedMacroSplicedUse {
  type Pair0 = (Option[Int], Option[String])
  type Pair1 = (Pair0, Pair0)
  type Pair2 = (Pair1, Pair1)
  type Pair3 = (Pair2, Pair2)
  type Sample = (Pair3, Pair3)

  val evidence: SplicedShape[Sample] = implicitly[SplicedShape[Sample]]
  def main(args: Array[String]): Unit = println(1)
}
