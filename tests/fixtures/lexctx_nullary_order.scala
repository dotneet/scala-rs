object Main {
 var n=0
 def f:()=>String={n+=1;()=>{n+=10;"value"}}
 def explicit():()=>Int={n+=100;()=>{n+=1000;n}}
 def nested:()=>()=>Int=()=>()=>5
 def main(args:Array[String]):Unit={
  println(f());println(n)
  val g=explicit();println(n);println(g())
  println(explicit()());println(nested()())
 }
}
