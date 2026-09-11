class Index {def apply(i:Int):Int=i+7}
object Values {def x():Index=new Index}
object Main {def main(args:Array[String]):Unit=println(Values.x(0))}
