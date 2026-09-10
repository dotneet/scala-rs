object Main {def main(args:Array[String]):Unit={val x=LazyList(1,2,3);println(x.mkString);println(x.mkString(","));println(x.mkString("[",",","]"))}}
