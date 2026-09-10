trait AB {
 type TA <: A
 type TB <: B
 trait A {val entities:List[TB]}
 trait B {var group:TA}
}
object N extends AB {
 type TA=NA;type TB=NB
 class NA extends A {val entities=List[NB]()}
 class NB extends B {var group=new NA}
}
object Main {def main(args:Array[String]):Unit={
 val b=new N.NB; println(b.group.entities.size);b.group=new N.NA;println(b.group.entities.size)
}}
