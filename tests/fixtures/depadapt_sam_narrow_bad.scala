object Main { val c:java.util.function.Consumer[AnyRef]=(s:String)=>println(s.length);def main(args:Array[String]):Unit=c.accept(new Object) }
