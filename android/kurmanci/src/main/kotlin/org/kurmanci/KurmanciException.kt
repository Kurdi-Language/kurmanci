package org.kurmanci

open class KurmanciException(
    message: String,
    cause: Throwable? = null
) : RuntimeException(message, cause) {

    class InvalidArgumentException(message: String, cause: Throwable? = null) : KurmanciException(message, cause)
    class InvalidPackException(message: String, cause: Throwable? = null) : KurmanciException(message, cause)
    class IncompatiblePackException(message: String, cause: Throwable? = null) : KurmanciException(message, cause)
    class IoException(message: String, cause: Throwable? = null) : KurmanciException(message, cause)
    class NativeException(message: String, cause: Throwable? = null) : KurmanciException(message, cause)
}
