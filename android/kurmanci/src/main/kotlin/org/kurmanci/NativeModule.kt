package org.kurmanci

internal object NativeModule {
    init {
        try {
            System.loadLibrary("kurmanci_jni")
        } catch (e: UnsatisfiedLinkError) {
            throw KurmanciException.NativeException(
                "Failed to load native library libkurmanci_jni.so: ${e.message}",
                e
            )
        }
    }

    @JvmStatic
    external fun nativeCreate(packData: ByteArray): Long

    @JvmStatic
    external fun nativeCreatePath(path: String): Long

    @JvmStatic
    external fun nativeDestroy(handle: Long)

    @JvmStatic
    external fun nativeGetPackInfo(handle: Long): PackInfo

    @JvmStatic
    external fun nativeIsKnownWord(handle: Long, word: String): Boolean

    @JvmStatic
    external fun nativeSuggest(handle: Long, query: String, limit: Int): Array<Candidate>?

    @JvmStatic
    external fun nativeComplete(handle: Long, prefix: String, limit: Int): Array<Candidate>?

    @JvmStatic
    external fun nativeCorrect(handle: Long, input: String, limit: Int): Array<Candidate>?

    @JvmStatic
    external fun nativePredict(handle: Long, contextWords: Array<String>, limit: Int): Array<PredictionCandidate>?
}
