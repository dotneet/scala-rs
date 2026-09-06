object Main {
  def f[T >: String](x:Any)(implicit ct:reflect.ClassTag[T]):String=ct.toString
  def main(args:Array[String]):Unit=println(f(1))
}
