// The nested interface really does declare `getKey` / `getValue` / `setValue`,
// and nothing up the chain implements them: the "needs to be abstract" check
// must still fire. (`equals` / `hashCode`, which `Entry` re-declares deferred
// per JLS 9.2, are implemented by `java.lang.Object` and must *not* be listed.)
class Half extends java.util.Map.Entry[String, Int] {
  def getKey(): String = "k"
}

object Main {
  def main(args: Array[String]): Unit = println(new Half().getKey())
}
