package org.kurmanci.consumer

import android.os.Build
import android.system.Os
import android.system.OsConstants
import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.kurmanci.KurmanciEngine
import java.io.File
import java.security.MessageDigest

/**
 * Internal device benchmark harness (not a consumer feature): loads the bundled
 * `benchmark_pack.bin`, records load time, resident memory, per-operation latency for
 * known / suggest / correct / complete / predict, and repeated-query stability, and logs one
 * JSON line prefixed `KURMANCI_DEVICE_BENCHMARK` (tag `KurmanciDeviceBenchmark`) that
 * `scripts/android/device-benchmark.sh` collects; the same JSON is written to the app's
 * external files directory. The committed `benchmark_pack.bin` is a tiny placeholder; the
 * script swaps a real pack in for a measurement run. No timing assertion: numbers are
 * recorded, only stability (identical results across repetitions) is asserted.
 */
@RunWith(AndroidJUnit4::class)
class DeviceBenchmarkTest {

    private fun residentBytes(): Long {
        val statm = File("/proc/self/statm").readText().trim().split(" ")
        val residentPages = statm.getOrNull(1)?.toLongOrNull() ?: return 0
        return residentPages * Os.sysconf(OsConstants._SC_PAGESIZE)
    }

    private fun percentile(sorted: List<Double>, p: Double): Double {
        if (sorted.isEmpty()) return 0.0
        val rank = minOf(sorted.size - 1, Math.round((sorted.size - 1) * p).toInt())
        return sorted[rank]
    }

    @Test
    fun testDeviceBenchmarkReport() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val bytes = instrumentation.context.assets.open("benchmark_pack.bin").use { it.readBytes() }
        val packSha256 = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

        // Load time: cold loads of the same bytes.
        val loadsMs = (0 until 5).map {
            val start = System.nanoTime()
            KurmanciEngine.open(bytes).use { it.packInfo }
            (System.nanoTime() - start) / 1_000_000.0
        }.sorted()

        KurmanciEngine.open(bytes).use { engine ->
            val info = engine.packInfo
            val rssAfterLoad = residentBytes()

            val iterations = 200
            data class Op(val name: String, val input: String, val run: () -> Int)
            val ops = listOf(
                Op("known_hit", "welat") { if (engine.isKnownWord("welat")) 1 else 0 },
                Op("known_miss", "xyzqwv") { if (engine.isKnownWord("xyzqwv")) 1 else 0 },
                Op("suggest", "rojbas") { engine.suggest("rojbas", 5).candidates.size },
                Op("correct", "spaz") { engine.correct("spaz", 5).candidates.size },
                Op("complete", "ro") { engine.complete("ro", 5).candidates.size },
                Op("predict", "ez") { engine.predictNextWord(listOf("ez"), 5).candidates.size },
            )
            val opReports = JSONArray()
            for (op in ops) {
                val samples = ArrayList<Double>(iterations)
                var count = 0
                repeat(iterations) {
                    val start = System.nanoTime()
                    count = op.run()
                    samples.add((System.nanoTime() - start) / 1000.0)
                }
                samples.sort()
                opReports.put(
                    JSONObject()
                        .put("name", op.name)
                        .put("input", op.input)
                        .put("iterations", iterations)
                        .put("p50_us", percentile(samples, 0.5))
                        .put("p95_us", percentile(samples, 0.95))
                        .put("max_us", samples.last())
                        .put("result_count", count)
                )
            }

            // Repeated-query stability: the same inputs must give identical results every time.
            fun snapshot(): String = listOf(
                engine.isKnownWord("welat").toString(),
                engine.suggest("rojbas", 5).candidates.joinToString(",") { "${it.text}:${it.editCost}" },
                engine.correct("spaz", 5).candidates.joinToString(",") { "${it.text}:${it.editCost}" },
                engine.complete("ro", 5).candidates.joinToString(",") { it.text },
                engine.predictNextWord(listOf("ez"), 5).candidates.joinToString(",") { "${it.text}:${it.count}" },
            ).joinToString("|")
            val reference = snapshot()
            val rounds = 300
            var stable = true
            for (round in 0 until rounds) {
                if (snapshot() != reference) {
                    stable = false
                    break
                }
            }
            val rssAfterQueries = residentBytes()

            val report = JSONObject()
                .put("schema_version", "device-benchmark-v1")
                .put("platform", "android")
                .put("device_model", "${Build.MANUFACTURER} ${Build.MODEL}")
                .put("os_version", "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
                .put("abi", Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown")
                .put("simulator", Build.FINGERPRINT.contains("generic") || Build.MODEL.contains("sdk", ignoreCase = true) || Build.HARDWARE.contains("ranchu"))
                .put("pack_file", "benchmark_pack.bin")
                .put("pack_bytes", bytes.size)
                .put("pack_sha256", packSha256)
                .put("entry_count", info.entryCount)
                .put("pack_format_version", info.formatVersion)
                .put("load_ms_median", percentile(loadsMs, 0.5))
                .put("load_ms_min", loadsMs.first())
                .put("load_ms_max", loadsMs.last())
                .put("rss_after_load_bytes", rssAfterLoad)
                .put("rss_after_queries_bytes", rssAfterQueries)
                .put("operations", opReports)
                .put("stability_rounds", rounds)
                .put("stable", stable)
            val json = report.toString()
            Log.i("KurmanciDeviceBenchmark", "KURMANCI_DEVICE_BENCHMARK $json")
            val outDir = instrumentation.targetContext.getExternalFilesDir(null)
                ?: instrumentation.targetContext.filesDir
            File(outDir, "kurmanci-device-benchmark.json").writeText(json)
            assertTrue("repeated queries returned different results", stable)
        }
    }
}
