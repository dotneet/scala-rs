package shapeprobe

object NestedMacroUse {
  type Pair0 = (Option[Int], Option[String])
  type Pair1 = (Pair0, Pair0)
  type Pair2 = (Pair1, Pair1)
  type Pair3 = (Pair2, Pair2)
  type Pair4 = (Pair3, Pair3)
  type Pair5 = (Pair4, Pair4)
  type Pair6 = (Pair5, Pair5)
  type Sample = (Pair6, Pair6)

  val evidence: NestedShape[Sample] = implicitly[NestedShape[Sample]]
  def main(args: Array[String]): Unit = println(1)
}
