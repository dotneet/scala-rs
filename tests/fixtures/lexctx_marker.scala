object Main { def main(args:Array[String]):Unit = {
 val marker: AnyRef = scala.runtime.Statics.pfMarker
 println(marker ne null)
 println(marker eq scala.runtime.Statics.pfMarker)
}}
