package org.kurmanci.consumer

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.kurmanci.KurmanciEngine
import org.kurmanci.KurmanciException
import org.kurmanci.PackInfo

class JvmUnitTest {

    @Test
    fun testPackInfoDataClass() {
        val info = PackInfo(languageTag = "kmr-Latn", formatVersion = 4, entryCount = 100L)
        assertEquals("kmr-Latn", info.languageTag)
        assertEquals(4, info.formatVersion)
        assertEquals(100L, info.entryCount)
    }

    @Test
    fun testExceptionHierarchy() {
        val ex: KurmanciException = KurmanciException.InvalidArgumentException("Invalid argument")
        assertTrue(ex is KurmanciException.InvalidArgumentException)
        assertEquals("Invalid argument", ex.message)
    }

    @Test
    fun testEmptyBytesThrows() {
        try {
            KurmanciEngine.open(ByteArray(0))
            fail("Expected IllegalArgumentException for empty byte array")
        } catch (e: IllegalArgumentException) {
            assertTrue(e.message?.contains("empty") == true)
        }
    }
}
