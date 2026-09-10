object Main {
 class Dep(x:Int)(implicit val nameClash:String) { def result:String=nameClash+":"+x }
 implicit val nameClash:String="outer"
 def meth(implicit w:String):Int=w.length
 class Meh extends Dep(meth)
 def main(args:Array[String]):Unit=println(new Meh().result)
}
