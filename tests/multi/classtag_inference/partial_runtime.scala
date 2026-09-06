trait W[T]
object Main {
  implicit val w:W[Int]=new W[Int] {}
  def f[T,U](x:Any)(implicit ev:W[T], ct:reflect.ClassTag[U]):String=ct.toString
  def main(args:Array[String]):Unit=println(f(1))
}
