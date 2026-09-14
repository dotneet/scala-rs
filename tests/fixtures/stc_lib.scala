package stc

class SlickTreeException(
    val msg: String,
    val detail: String,
    val parent: Throwable = null,
    val mark: String => Boolean = null,
    val removeUnmarked: Boolean = true
)
