// The leaf. `SchemaDescriptionDef` is inherited from a trait this file never
// names, and the nested class's *constructor parameter* names the alias --
// which is what the header pass types, forcing the alias to be completed
// before the parent chain is known. With the first round's scope frozen in,
// the alias's right-hand side stayed an unresolved name and `new DDL(...)`
// was `found: DDL  required: SchemaDescriptionDef` (slick's MemoryProfile).
package mism12.memory

import mism12.relational.RelationalProfile

trait MemoryProfile extends RelationalProfile { self: MemoryProfile =>
  type SchemaDescription = SchemaDescriptionDef

  def buildTableSchemaDescription(name: String): SchemaDescription = new DDL(Vector(name))

  def combine(a: SchemaDescription, b: SchemaDescription): SchemaDescription =
    new DDL(a.asInstanceOf[DDL].tables ++ b.asInstanceOf[DDL].tables)

  class DDL(val tables: Vector[String]) extends SchemaDescriptionDef {
    def show: String = tables.mkString("[", ",", "]")
  }

  class SchemaActionImpl(schema: SchemaDescription) {
    def create: String = "create " + schema.show
  }
}

object MemoryProfile extends MemoryProfile
