package nestedcasepickle

object Outer {
  case class Status(
      url: Option[String],
      enforcement_level: String,
      contexts: Seq[String],
      contexts_url: Option[String]
  )
}
