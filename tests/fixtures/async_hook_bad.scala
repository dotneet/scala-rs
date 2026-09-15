import asynchook.Generic
object Main {
  val valid = Generic.optionally { 42 }
  val outside = Generic.value(Some(42))
  val nested = Generic.optionally { () => Generic.value(Some(42)) }
}
