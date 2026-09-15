package mirrorfixture {
 class Outer { type Alias = String; class Nested(val x: Int); object Marker; private[mirrorfixture] val value: Int = 1 }
}
object Main { def main(args: Array[String]): Unit = println(mirrorprobe.MirrorFeatures.inspect[mirrorfixture.Outer]) }
