class Yo(val tag:String) { def yo():String=tag }
object Main {
  implicit def view[T](a:Any)(implicit ct:reflect.ClassTag[T]):Yo=new Yo(ct.toString)
  def f[T](x:Any)(implicit ct:reflect.ClassTag[T]):String=ct.toString
  def main(args:Array[String]):Unit={println("".yo());println(f(1))}
}
