import scala.reflect.ClassTag
object Main {def show(x:AnyRef):String="ref";
def show(x:Any)(implicit c:ClassTag[Any]):String="any";
def main(args:Array[String]):Unit={println(show("s"));
println(show(3));
println(show(null));
println(show(true))}}
