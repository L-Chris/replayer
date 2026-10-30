package dev.replayer

import dev.replayer.comic.KomiicAccount
import java.io.File
import android.content.Context
import dev.replayer.sync.CloudSync
import dev.replayer.sync.SecretStore
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import java.util.UUID
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test

class KomiicAccountTest {
    @Test
    fun savedAccountRestoresFromTheVaultAndLogoutClearsIt() = runBlocking {
        assertTrue(Native.initialize(testContext))
        val previous = KomiicAccount.saved(testContext)
        val prefs = testContext.getSharedPreferences("komiic", Context.MODE_PRIVATE)
        val email = "fixture-${UUID.randomUUID()}@example.test"
        val password = "fixture-private-password"
        val key = "komiic/${CloudSync.sha256(email)}/password"
        try {
            SecretStore.put(testContext, mapOf(key to password))
            check(prefs.edit().putString("email", email).putBoolean("auto_login", true).commit())
            KomiicAccount.initialize(testContext)
            assertTrue(KomiicAccount.saved(testContext).optString("password") == password)
            assertEquals("pending", JSONObject(Native.nativeKomiicStatus()).getString("phase"))
            assertFalse(prefs.all.toString().contains(password))
            // Pending sessions never contacted the site: this fixture stays offline.
            withTimeout(10_000) { KomiicAccount.logout(testContext).join() }
            val saved = KomiicAccount.saved(testContext)
            assertEquals(email, saved.getString("email"))
            assertFalse(saved.getBoolean("auto_login"))
            assertTrue(saved.getString("password").isEmpty())
            assertEquals("guest", JSONObject(Native.nativeKomiicStatus()).getString("phase"))
        } finally {
            SecretStore.put(testContext, mapOf(key to ""))
            check(prefs.edit().putString("email", previous.optString("email"))
                .putBoolean("auto_login", previous.optBoolean("auto_login")).commit())
            Native.nativeKomiicConfigure(previous.toString())
        }
    }

    @Test
    fun configurationValidatesBeforeReplacingTheSession() {
        assertTrue(Native.initialize(testContext))
        val previous = KomiicAccount.saved(testContext)
        try {
            Native.nativeKomiicConfigure("{}")
            assertEquals("guest", JSONObject(Native.nativeKomiicStatus()).getString("phase"))
            assertThrows(IllegalStateException::class.java) {
                Native.nativeKomiicConfigure(JSONObject().put("email", "bad").put("auto_login", true).toString())
            }
            assertEquals("guest", JSONObject(Native.nativeKomiicStatus()).getString("phase"))
            val status = Native.nativeKomiicConfigure(JSONObject().put("email", "reader@example.test")
                .put("password", "fixture-secret").put("auto_login", true).toString())
            assertEquals("pending", JSONObject(status).getString("phase"))
            assertFalse(status.contains("fixture-secret"))
            assertFalse(status.contains("reader@example.test"))
        } finally { Native.nativeKomiicConfigure(previous.toString()) }
    }

    @Test
    fun explicitlySuppliedLiveCredentialsSignInThroughTheNativeBridge() {
        assumeTrue("requires --online and a private fixture", onlineRequested())
        val fixture = File(testContext.cacheDir, "komiic-account-fixture.json")
        assumeTrue("requires a private account fixture; never bundle credentials in the APK", fixture.isFile)
        assertTrue(Native.initialize(testContext))
        val previous = KomiicAccount.saved(testContext)
        try {
            // Supplied through stdin into run-as private cache, never instrumentation args/logs.
            Native.nativeKomiicConfigure(fixture.readText())
            assertEquals("signed_in", JSONObject(Native.nativeKomiicLogin()).getString("phase"))
            assertEquals("signed_in", JSONObject(Native.nativeKomiicStatus()).getString("phase"))
        } finally {
            fixture.delete()
            // Discard this session locally without logging out other clients.
            Native.nativeKomiicConfigure(previous.toString())
        }
    }
}
