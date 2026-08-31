package mism12.use

import mism12.memory.MemoryProfile

object Main {
  def main(args: Array[String]): Unit = {
    val p = MemoryProfile
    val a = p.buildTableSchemaDescription("people")
    println(p.describe(a))
    println(new p.SchemaActionImpl(a).create)
    println(p.combine(a, p.buildTableSchemaDescription("orders")).show)
  }
}
