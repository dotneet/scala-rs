object Main {def main(args:Array[String]):Unit={val x:IterableOnce[Int]=Iterator(1,2,3);println(x.reduceOption(_+_))}}
