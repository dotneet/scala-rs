trait Box {def value:Any};object N extends Box {val value=7};object Main {def main(args:Array[String]):Unit={val b:Box=N;println(b.value)}}
