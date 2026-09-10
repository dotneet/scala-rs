import scala.reflect.ClassTag
object Main{def use(x:Any):Int=1;
def use(x:Any)(implicit c:ClassTag[String]):Int=2;
val x=use(3)}
