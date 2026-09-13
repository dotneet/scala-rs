import seqpickle.SeqRecord

object Main {
  def main(args: Array[String]): Unit = {
    val record = SeqRecord(owner = "owner", values = List.empty[String])
    println(record.owner + ":" + record.values.size)
  }
}
