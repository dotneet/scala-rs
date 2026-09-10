object Main {def main(args:Array[String]):Unit={
 val o=new ObjectFields[String];o.array(0)="changed";println(o.array(0))
 o.array=Array[AnyRef]("reset");println(o.array(0))
 o.array(0)=7;val ref:AnyRef=o.array(0);println(ref)
 val a=o.array;a(0)=8;val alias:AnyRef=a(0);println(alias)
}}
