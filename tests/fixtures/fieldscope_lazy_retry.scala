class Cell[T](init:()=>T) {lazy val value:T=init()}
object Main {var tries=0;def main(args:Array[String]):Unit={
 val c=new Cell[Array[Int]](()=>{tries+=1;if(tries==1) throw new IllegalStateException("retry");Array(7)})
 try {println(c.value(0))} catch {case _:IllegalStateException=>println("retry")}
 println(c.value(0));println(c.value(0));println(tries)
}}
