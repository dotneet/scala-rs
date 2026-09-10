object Main {def main(args:Array[String]):Unit={
 val marker:AnyRef=ObjectFields.MARKER
 println(marker eq ObjectFields.MARKER)
 ObjectFields.shared=1;println(ObjectFields.shared)
 val o=new ObjectFields[String]
 o.value=2;val ref:AnyRef=o.value;println(ref)
 o.generic="g";val generic:AnyRef=o.generic;println(generic)
 println(ObjectFields.echo(3))
 val r:AnyRef=ObjectFields.echo(4);println(r)
}}
