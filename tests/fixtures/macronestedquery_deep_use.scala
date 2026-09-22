object Main {
  type B01 = DeepBox[Int]
  type B02 = DeepBox[B01]
  type B03 = DeepBox[B02]
  type B04 = DeepBox[B03]
  type B05 = DeepBox[B04]
  type B06 = DeepBox[B05]
  type B07 = DeepBox[B06]
  type B08 = DeepBox[B07]
  type B09 = DeepBox[B08]
  type B10 = DeepBox[B09]
  type B11 = DeepBox[B10]
  type B12 = DeepBox[B11]
  type B13 = DeepBox[B12]
  type B14 = DeepBox[B13]
  type B15 = DeepBox[B14]
  type B16 = DeepBox[B15]
  type B17 = DeepBox[B16]

  def main(args: Array[String]): Unit = println(DeepQuery.value[B17])
}
