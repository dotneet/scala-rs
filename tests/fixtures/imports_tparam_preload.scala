object ImportsTypeParamPreload {
  // Complete the external generic method before the following source unit's
  // wildcard import is entered. This pins the classpath lazy-load order.
  val loaded = imported.Exports.noisy[Int, String](1, "x")
}
