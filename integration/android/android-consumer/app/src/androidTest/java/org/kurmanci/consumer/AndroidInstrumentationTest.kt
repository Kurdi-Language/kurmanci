package org.kurmanci.consumer

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.junit.runner.RunWith
import org.kurmanci.KurmanciEngine
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

@RunWith(AndroidJUnit4::class)
class AndroidInstrumentationTest {

    private fun loadAssetBytes(filename: String): ByteArray {
        val context = InstrumentationRegistry.getInstrumentation().context
        return context.assets.open(filename).use { it.readBytes() }
    }

    @Test
    fun testRealEngineIntegration() {
        val packBytes = loadAssetBytes("apple_consumer_test.bin")

        KurmanciEngine.open(packBytes).use { engine ->
            // 1. Pack Info Verification
            val info = engine.packInfo
            assertNotNull(info)
            assertEquals(4, info.formatVersion)
            assertTrue("Expected entries > 0", info.entryCount > 0)

            // 2. Known Word Lookup
            assertTrue("Expected 'welat' to be known", engine.isKnownWord("welat"))

            // 3. Correction & Completion APIs (Exact Assertions)
            val corrections = engine.correct("spaz", 5)
            assertTrue("Expected corrections for 'spaz'", corrections.candidates.isNotEmpty())
            val topCorr = corrections.candidates.first()
            assertEquals("spas", topCorr.text)
            assertEquals(10, topCorr.editCost)

            val completions = engine.complete("roj", 5)
            assertTrue("Expected completions for 'roj'", completions.candidates.isNotEmpty())
            val topComp = completions.candidates.first()
            assertEquals("roj", topComp.text)

            // 4. Clamping & Options
            val emptySuggest = engine.suggest("welat", 1)
            assertNotNull(emptySuggest)

            // 5. Post-close exception check
            engine.close()
            try {
                engine.isKnownWord("welat")
                fail("Expected IllegalStateException after close")
            } catch (e: IllegalStateException) {
                assertTrue(e.message?.contains("closed") == true)
            }
        }
    }

    @Test
    fun testNextWordPredictionIntegration() {
        val predBytes = loadAssetBytes("prediction_test.bin")

        KurmanciEngine.open(predBytes).use { engine ->
            val predictions = engine.predictNextWord(listOf("ez"), 5)
            assertTrue("Expected predictions for 'ez'", predictions.candidates.isNotEmpty())

            val top = predictions.candidates.first()
            assertEquals("ji", top.text)
            assertEquals(3L, top.count)
            assertEquals("Expected prediction source to be bigram (2)", 2, top.source)
        }
    }

    @Test
    fun testCloseIsIdempotent() {
        val packBytes = loadAssetBytes("apple_consumer_test.bin")
        val engine = KurmanciEngine.open(packBytes)

        engine.close()
        engine.close()
        engine.close()

        try {
            engine.packInfo
            fail("Expected IllegalStateException after close")
        } catch (e: IllegalStateException) {
            assertTrue(e.message?.contains("closed") == true)
        }
    }

    @Test
    fun testAllQueriesFailAfterClose() {
        val packBytes = loadAssetBytes("apple_consumer_test.bin")
        val engine = KurmanciEngine.open(packBytes)
        engine.close()

        val actions: List<Pair<String, () -> Unit>> = listOf(
            "packInfo" to { engine.packInfo },
            "isKnownWord" to { engine.isKnownWord("welat") },
            "suggest" to { engine.suggest("welat", 5) },
            "complete" to { engine.complete("roj", 5) },
            "correct" to { engine.correct("spaz", 5) },
            "predictNextWord" to { engine.predictNextWord(listOf("ez"), 5) }
        )

        for ((name, action) in actions) {
            try {
                action()
                fail("Expected IllegalStateException for $name after close")
            } catch (e: IllegalStateException) {
                assertTrue("Expected closed message for $name", e.message?.contains("closed") == true)
            }
        }
    }

    @Test
    fun testConcurrentReadQueries() {
        val packBytes = loadAssetBytes("apple_consumer_test.bin")
        val engine = KurmanciEngine.open(packBytes)

        val threadCount = 10
        val iterationsPerThread = 50
        val executor = Executors.newFixedThreadPool(threadCount)
        val latch = CountDownLatch(threadCount)
        val successCount = AtomicInteger(0)

        for (i in 0 until threadCount) {
            executor.submit {
                try {
                    for (j in 0 until iterationsPerThread) {
                        val known = engine.isKnownWord("welat")
                        val corr = engine.correct("spaz", 3)
                        val comp = engine.complete("roj", 3)
                        if (known && corr.candidates.isNotEmpty() && comp.candidates.isNotEmpty()) {
                            successCount.incrementAndGet()
                        }
                    }
                } finally {
                    latch.countDown()
                }
            }
        }

        assertTrue("Timeout waiting for concurrent query threads", latch.await(10, TimeUnit.SECONDS))
        executor.shutdown()
        assertEquals(threadCount * iterationsPerThread, successCount.get())
        engine.close()
    }

    @Test
    fun testConcurrentQueryAndCloseStress() {
        val packBytes = loadAssetBytes("apple_consumer_test.bin")
        val engine = KurmanciEngine.open(packBytes)

        val queryThreads = 8
        val executor = Executors.newFixedThreadPool(queryThreads + 1)
        val startLatch = CountDownLatch(1)
        val doneLatch = CountDownLatch(queryThreads + 1)
        val querySuccesses = AtomicInteger(0)
        val closedExceptions = AtomicInteger(0)

        for (i in 0 until queryThreads) {
            executor.submit {
                startLatch.await()
                for (j in 0 until 200) {
                    try {
                        if (engine.isKnownWord("welat")) {
                            querySuccesses.incrementAndGet()
                        }
                    } catch (e: IllegalStateException) {
                        if (e.message?.contains("closed") == true) {
                            closedExceptions.incrementAndGet()
                        } else {
                            throw e
                        }
                    }
                }
                doneLatch.countDown()
            }
        }

        executor.submit {
            startLatch.await()
            Thread.sleep(5)
            engine.close()
            doneLatch.countDown()
        }

        startLatch.countDown()
        assertTrue("Timeout in stress test", doneLatch.await(10, TimeUnit.SECONDS))
        executor.shutdown()

        assertTrue("Total query results must equal thread total", querySuccesses.get() + closedExceptions.get() == queryThreads * 200)
    }
}
