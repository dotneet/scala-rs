package xmetaprofile

trait BasicProfile {
  type Schema <: SchemaDef

  trait SchemaDef {
    def ++(other: Schema): Schema
  }

  object api {
    type Item[A] = Schema
    def one: Schema = make
  }

  def make: Schema
}

trait RelationalProfile extends BasicProfile {
  type Schema = DDL

  class DDL extends SchemaDef {
    def ++(other: Schema): Schema = this
  }

  def make: Schema = new DDL
}

object ConcreteProfile extends RelationalProfile
