trait H { val f:()=>Int }; object Main {def main(args:Array[String]):Unit={val build=(s:Int)=>{class C extends H{val f=()=>s};new C};println(build(8).f())}}
