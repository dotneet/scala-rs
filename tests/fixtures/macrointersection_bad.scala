object BadIntersection {
  val value: java.io.Serializable with IntersectionMarker["other"] = IntersectionMacro.value
}
