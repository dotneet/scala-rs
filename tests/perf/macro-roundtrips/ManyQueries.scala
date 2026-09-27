object ManyQueries {
  def main(args: Array[String]): Unit = println(
    Probe.repeat(400) + Probe.repeat(400) + Probe.repeat(400) + Probe.repeat(400) +
    Probe.repeat(400) + Probe.repeat(400) + Probe.repeat(400) + Probe.repeat(400) +
    Probe.repeat(400) + Probe.repeat(400) + Probe.repeat(400) + Probe.repeat(400) +
    Probe.repeat(400) + Probe.repeat(400) + Probe.repeat(400) + Probe.repeat(400)
  )
}
