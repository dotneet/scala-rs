class StoragePrivateValue(private val value: Int) extends AnyVal
class StorageAccessorBase(protected[this] val seed: Int) {
  protected[this] val (left, right) = (seed, seed + 1)
  protected[this] var pos = left
  def advance(): Int = { pos += right; pos }
}
class StorageAccessorChild extends StorageAccessorBase(3)
object StorageAccessorApi {
  def echo(v: StoragePrivateValue): StoragePrivateValue = v
}
class StorageCaptured(seed: Int) { def reader: () => Int = () => seed }
class StoragePrivateCaptured {
  private[this] var count: Int = 5
  def reader: () => Int = () => { count += 1; count }
}
