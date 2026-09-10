class Index {def apply(i:Int):Int=i+7}
object Values {
 def array:Array[Int]=Array(7)
 def empty():Array[Int]=Array(8)
 def index:Index=new Index
}
object Main {def main(args:Array[String]):Unit={
 println(Values.array(0));println(Values.empty()(0));println(Values.index(0));println("abc".toCharArray()(0))
}}
