object Main{var count=0;def twice(body: => Unit):Unit={body;body};def main(args:Array[String]):Unit=twice{val row=List("a","b").map{x=>count+=1;x->count}.toMap;println(row("b"));println(count)}}
