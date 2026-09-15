import compiledquery._

object Main {
  implicit val shape: Shape[ShapeLevel, (Int, Int), (Int, Int), (Int, Int)] = new Shape[ShapeLevel, (Int, Int), (Int, Int), (Int, Int)] {
    override type Packed = (Int, Int)
  }
  implicit val intShape: Shape[ShapeLevel, Int, Int, Int] = new Shape[ShapeLevel, Int, Int, Int] {}
  implicit val stringShape: Shape[ShapeLevel, String, String, String] = new Shape[ShapeLevel, String, String, String] {}
  val packedStringShape: Shape[ShapeLevel, Int, Int, String] = new Shape[ShapeLevel, Int, Int, String] {}
  val packedIntShape: Shape[ShapeLevel, Int, Int, Int] = new Shape[ShapeLevel, Int, Int, Int] {}

  val parameters = Parameters[(Int, Int)]
  val filtered = parameters.withFilter(_ => true)
  val nested = Parameters[(Int, (Int, String))]
  val nestedFiltered = nested.withFilter(_ => true)
  def firstPacked: Selected[String] = Parameters.firstPacked(packedStringShape, packedIntShape)
  def secondPacked: Selected[Int] = Parameters.secondPacked(packedStringShape, packedIntShape)
  val ownerOne: ownerone.Shape = new ownerone.Shape {}
  val ownerTwo: ownertwo.Shape = new ownertwo.Shape {}
  def ownerSelected: Selected[String] = OwnerProbe.first(ownerOne, ownerTwo)
  val fixed: inherited.Fixed = new inherited.Fixed
  def inheritedSelected: Selected[String] = inherited.select(fixed)
  def main(args: Array[String]): Unit = println(
    (filtered eq parameters) &&
      (nestedFiltered eq nested) &&
      (firstPacked ne null) &&
      (secondPacked ne null) &&
      (ownerSelected ne null) &&
      (inheritedSelected ne null)
  )
}
