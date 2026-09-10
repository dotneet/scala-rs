object Main { def main(args:Array[String]):Unit = {
 val a=Factory.make; val b=Factory.make
 println(a ne b); println(a.getClass != b.getClass)
} }
