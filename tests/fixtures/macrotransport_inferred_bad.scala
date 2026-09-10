object Bad { val result = InferredMacro.identity { val n: Int = "wrong"; n } }
