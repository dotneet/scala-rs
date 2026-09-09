trait Required { def value: Int }
trait Other { def text: String }
trait SelfBound { self: Required =>
  def wrong: Other = this
}
class MissingRequirement extends SelfBound
