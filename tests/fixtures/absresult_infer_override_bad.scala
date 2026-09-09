trait Base { def method: Any; val field: Any }
class Impl extends Base { def method = "method"; val field = "field" }
object Use {
  val method: String = new Impl().method
  val field: String = new Impl().field
}
