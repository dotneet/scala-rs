package slickstreaming

object SlickStreamingParent {
  def check(api: Api): Action[Seq[Int], NoStream, Nothing] = api.action
}
