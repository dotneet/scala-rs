object Main {
 def use(f:()=>String=>Int):Int=f()("abc")
 val bad=use(()=>(x:Int)=>x+1)
}
