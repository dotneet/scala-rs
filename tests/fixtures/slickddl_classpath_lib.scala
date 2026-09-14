package slickddlmini

trait Profile {
  type SchemaDescription <: SchemaDescriptionDef
  trait SchemaDescriptionDef {
    def show: String
  }
  trait API {
    implicit def schemaActionExtensionMethods(
        sd: SchemaDescription
    ): SchemaActionExtensionMethods
  }
  val api: API
  class SchemaActionExtensionMethods(sd: SchemaDescription) {
    def create: String = sd.show
  }
}
