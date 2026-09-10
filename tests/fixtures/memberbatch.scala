trait BatchParent[A] { def payload:A }
class BatchBase extends BatchParent[String] { def name:String="base";def payload:String="abc" }
class BatchChild extends BatchBase {def childOnly:Int=1}
object Main {
  implicit class Rich(private val s:String) extends AnyVal {
    def replaceAll(p:String)(f:String=>String):String=f(s)
  }
  def read(xs:List[_ <: BatchBase]):String=xs.head.name+xs.head.payload
  def pairs[A,B](xs:Array[(A,B)]):Map[A,B]=xs.toMap
  def main(args:Array[String]):Unit = {
    println(read(List(new BatchChild)))
    println("abc".replaceAll("b")((x:String)=>x+"!"))
    println("abc".replaceAll("b","d"))
    println(pairs(Array((1,"a"))))
    try {throw new RuntimeException("caught")} catch {case e if e.getMessage == "caught" => val t:Throwable=e;println(t.getMessage)}
    try {throw new RuntimeException("nonfatal")} catch {case scala.util.control.NonFatal(e) => println(e.getMessage)}
  }
}
