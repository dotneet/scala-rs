trait Box {type T;var value:T};class N extends Box {type T=Int;var value=7};object Main {def main(args:Array[String]):Unit={val b:Box{type T=Int}=new N;println(b.value);b.value=9;println(b.value)}}
