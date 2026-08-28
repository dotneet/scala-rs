package cake.relational

trait BasicProfile {
  def profileName: String
}

/** The middle of the cake: it mixes the components in but declares nothing
  * of its own. */
trait RelationalProfile
  extends BasicProfile
    with RelationalTableComponent
    with RelationalSequenceComponent { self: RelationalProfile =>
}
