package org.kurmanci

import java.io.Closeable
import java.util.concurrent.locks.ReentrantReadWriteLock
import kotlin.concurrent.read
import kotlin.concurrent.write

class KurmanciEngine private constructor(
    private var handle: Long
) : Closeable {

    private val lock = ReentrantReadWriteLock()

    private inline fun <T> withHandle(block: (Long) -> T): T {
        return lock.read {
            val h = handle
            check(h != 0L) { "KurmanciEngine instance has been closed." }
            block(h)
        }
    }

    val packInfo: PackInfo
        get() = withHandle { h ->
            NativeModule.nativeGetPackInfo(h)
        }

    fun isKnownWord(word: String): Boolean {
        require(word.isNotEmpty()) { "Word must not be empty" }
        return withHandle { h ->
            NativeModule.nativeIsKnownWord(h, word)
        }
    }

    fun suggest(query: String, maxCandidates: Int = 5): SuggestionResult {
        require(maxCandidates > 0) { "maxCandidates must be positive" }
        return withHandle { h ->
            val array = NativeModule.nativeSuggest(h, query, maxCandidates)
            SuggestionResult(array?.toList() ?: emptyList())
        }
    }

    fun complete(prefix: String, maxCandidates: Int = 5): SuggestionResult {
        require(maxCandidates > 0) { "maxCandidates must be positive" }
        return withHandle { h ->
            val array = NativeModule.nativeComplete(h, prefix, maxCandidates)
            SuggestionResult(array?.toList() ?: emptyList())
        }
    }

    fun correct(input: String, maxCandidates: Int = 5): SuggestionResult {
        require(maxCandidates > 0) { "maxCandidates must be positive" }
        return withHandle { h ->
            val array = NativeModule.nativeCorrect(h, input, maxCandidates)
            SuggestionResult(array?.toList() ?: emptyList())
        }
    }

    fun predictNextWord(contextWords: List<String>, maxCandidates: Int = 5): PredictionResult {
        require(maxCandidates > 0) { "maxCandidates must be positive" }
        return withHandle { h ->
            val array = NativeModule.nativePredict(h, contextWords.toTypedArray(), maxCandidates)
            PredictionResult(array?.toList() ?: emptyList())
        }
    }

    override fun close() {
        lock.write {
            val h = handle
            if (h != 0L) {
                handle = 0L
                NativeModule.nativeDestroy(h)
            }
        }
    }

    companion object {
        @JvmStatic
        fun open(bytes: ByteArray): KurmanciEngine {
            require(bytes.isNotEmpty()) { "Pack bytes must not be empty" }
            val handle = NativeModule.nativeCreate(bytes)
            if (handle == 0L) {
                throw KurmanciException.NativeException("Failed to initialize engine from byte array")
            }
            return KurmanciEngine(handle)
        }

        @JvmStatic
        fun openFile(path: String): KurmanciEngine {
            require(path.isNotEmpty()) { "File path must not be empty" }
            val handle = NativeModule.nativeCreatePath(path)
            if (handle == 0L) {
                throw KurmanciException.NativeException("Failed to initialize engine from file: $path")
            }
            return KurmanciEngine(handle)
        }
    }
}
