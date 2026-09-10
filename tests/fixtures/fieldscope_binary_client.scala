class FSIntCell extends FSCell[Int](1) {def bump():Int={value=7;value}}
class FSConcrete extends FSIntProperty {val value=9}
object Main {def main(args:Array[String]):Unit={
 val c=new FSCell[Array[Int]](Array(1));c.value=Array(7);println(c.value(0));println(c.cached(0))
 val d=new FSIntCell;println(d.bump());println(new FSConcrete().read)
 val e=new FSCell[Unit](());e.value=();println(e.value)
}}
