object Main {def main(args:Array[String]):Unit={println("abc".getBytes("UTF-8").mkString(","));println("é".getBytes(java.nio.charset.StandardCharsets.UTF_8).mkString(","))}}
