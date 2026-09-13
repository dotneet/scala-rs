trait ProtectedConfigBase {
  protected[this] def loadProfileConfig: String = "base"
  final def exposedConfig: String = loadProfileConfig
}

trait ProtectedConfigMiddle extends ProtectedConfigBase
trait UnrelatedMixin

trait ProtectedConfigProfile extends ProtectedConfigMiddle with UnrelatedMixin {
  override protected[this] def loadProfileConfig: String =
    "profile/" + super.loadProfileConfig
}

object Main extends ProtectedConfigProfile {
  def main(args: Array[String]): Unit = println(exposedConfig)
}
