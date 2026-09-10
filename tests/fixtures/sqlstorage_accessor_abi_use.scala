object Main {
  def main(args: Array[String]): Unit = {
    val child = new StorageAccessorChild
    println(child.advance())
    println(child.advance())
    val value = new StoragePrivateValue(value = 12)
    val id: StoragePrivateValue => StoragePrivateValue = x => x
    println(id(value) == value)
    println(StorageAccessorApi.echo(value) == value)
    val boxed: Any = value
    println(boxed match { case x: StoragePrivateValue => x == value; case _ => false })
    println(new StorageCaptured(seed = 8).reader.apply())
    println(new StoragePrivateCaptured().reader.apply())
    val values = List(value)
    println(values.head == value)
  }
}
