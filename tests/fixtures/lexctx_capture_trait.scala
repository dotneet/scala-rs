trait H {def value:Int}
object Main {
 val build=(n:Int)=>{trait T extends H {def value:Int=n}; class C extends T;new C}
 def main(args:Array[String]):Unit=println(build(9).value)
}
