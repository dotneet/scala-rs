import org.apache.pekko.NotUsed
import org.apache.pekko.stream.scaladsl.Flow

object PekkoFlowOpsProbe {
  val flow: Flow[Int, String, NotUsed] =
    Flow[Int].mapConcat(value => List(value.toString))
}
