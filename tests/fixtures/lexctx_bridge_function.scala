trait Box {type T;val value:T};object N extends Box {type T=Int=>Int;val value=(x:Int)=>x+1};object Main {def main(args:Array[String]):Unit={val b:Box{type T=Int=>Int}=N;println(b.value(8))}}
