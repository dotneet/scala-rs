// The same trait read from a class file.
object AppJ extends icl.CompA {
  type KA = Int; def mk: KA = 7
  def x: Int = new InnerA(mk).get
}
object Client {
  def main(args: Array[String]): Unit = {
    val b: Int = AppJ.mkI(3).get
    println((AppJ.x, b, AppJ.mkI(2).k + 1))
  }
}
