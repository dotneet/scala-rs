import scala.reflect.ClassTag
class Base[A] {def display(x: AnyVal):String="value";
def display(x:Any)(implicit ct:ClassTag[A]):String="any"}
object Main extends Base[String]{def main(args:Array[String]):Unit={println(display(List(1)));
println(display(42));
println(display(List(2))(ClassTag(classOf[String])));
println(java.lang.String.valueOf(true));
println(java.lang.String.valueOf(12))}}

