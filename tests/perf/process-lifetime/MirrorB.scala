package probe

object MirrorB {
  val value: Int = MirrorProvider.imported[MirrorA.type]
}
