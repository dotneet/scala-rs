trait H { val next:()=>Int }
class Factory {
 val build=(n:Int)=>{var x=n; new H {val next=()=>{x+=1;x}}}
}
object Main {val factory=new Factory; def main(args:Array[String]):Unit={
 val a=factory.build(2); val b=factory.build(10)
 println(a.next());println(b.next());println(a.next())
}}
