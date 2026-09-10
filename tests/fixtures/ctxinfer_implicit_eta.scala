object Main{def convert(x:Int)(implicit prefix:String):Option[String]=Some(prefix+x);
def main(args:Array[String]):Unit={implicit val prefix:String="n=";
println(Option(3).flatMap(convert))}}

