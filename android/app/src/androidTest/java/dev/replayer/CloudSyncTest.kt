package dev.replayer

import android.content.Context
import dev.replayer.sync.SecretStore
import dev.replayer.sync.CloudSync
import dev.replayer.music.MusicRepository
import dev.replayer.music.OnlineSong
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.flow.first
import java.net.InetAddress
import java.net.ServerSocket
import java.util.UUID
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.concurrent.thread
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class CloudSyncTest {
    private fun config(url: String): JSONObject = JSONObject().put("base_url", url)
        .put("username", "fixture-user").put("password", "fixture-password")
        .put("device_id", UUID.randomUUID().toString()).put("enabled", true)

    @Test
    fun settingsUseTheDesktopValidationRules() {
        assertTrue(Native.initialize(testContext))
        val valid = config("https://dav.example.invalid")
        assertTrue(JSONObject(Native.nativeSyncValidate(valid.toString())).getBoolean("valid"))
        assertThrows(IllegalStateException::class.java) {
            Native.nativeSyncValidate(config("http://dav.example.invalid").toString())
        }
        assertThrows(IllegalStateException::class.java) {
            Native.nativeSyncValidate(valid.put("sync_keys", true).put("passphrase", "short").toString())
        }
    }

    @Test
    fun credentialsAreEncryptedWithFreshNonces() {
        val name = "fixture/${UUID.randomUUID()}"
        val secret = "fixture-secret-${UUID.randomUUID()}"
        val prefs = testContext.getSharedPreferences("secrets", Context.MODE_PRIVATE)
        try {
            SecretStore.put(testContext, mapOf(name to secret))
            val first = prefs.getString("vault", null)
            assertEquals(secret, SecretStore.get(testContext, name))
            assertFalse(first!!.contains(secret))
            SecretStore.put(testContext, mapOf(name to secret))
            assertNotEquals(first, prefs.getString("vault", null))
            assertEquals(secret, SecretStore.get(testContext, name))
        } finally {
            SecretStore.put(testContext, mapOf(name to ""))
        }
    }

    @Test
    fun nativeWebdavProbeChecksAndRemovesItsOwnObject() {
        assertTrue(Native.initialize(testContext))
        WebDavFixture().use { server ->
            val result = JSONObject(Native.nativeSyncExchange(JSONObject()
                .put("config", config(server.url)).put("probe", true).toString()))
            assertTrue(result.getBoolean("tested"))
            assertEquals(listOf("MKCOL", "MKCOL", "MKCOL", "PROPFIND", "PUT", "GET", "DELETE"), server.methods.toList())
            assertTrue(server.files.isEmpty())
            assertTrue(server.errors.toString(), server.errors.isEmpty())
        }
    }

    @Test
    fun nativeRoundTripPreservesChangesAndPersistsTheJournal() {
        assertTrue(Native.initialize(testContext))
        WebDavFixture().use { server ->
            val config = config(server.url)
            val initial = JSONObject().put("music/source", "chksz-qq").put("music/quality", "lossless")
            val request = JSONObject().put("config", config).put("snapshot", initial).put("secrets", JSONObject())
            val exchange = JSONObject(Native.nativeSyncExchange(request.toString()))
            val file = java.io.File(testContext.filesDir, "sync/${exchange.getString("account")}/${exchange.getString("device")}.json")
            try {
                val latest = JSONObject(initial.toString()).put("music/source", "chksz-kugou")
                val result = JSONObject(Native.nativeSyncFinish(JSONObject().put("exchange", exchange)
                    .put("snapshot", latest).put("secrets", JSONObject()).toString()))
                assertEquals("chksz-kugou", result.getJSONObject("snapshot").getString("music/source"))
                assertTrue(result.getBoolean("pending"))
                assertTrue(file.exists())
                val again = JSONObject(Native.nativeSyncExchange(request.put("snapshot", latest).toString()))
                val finished = JSONObject(Native.nativeSyncFinish(JSONObject().put("exchange", again)
                    .put("snapshot", latest).put("secrets", JSONObject()).toString()))
                assertFalse(finished.getBoolean("pending"))
                assertEquals("chksz-kugou", finished.getJSONObject("snapshot").getString("music/source"))
                assertTrue(server.errors.toString(), server.errors.isEmpty())
            } finally {
                file.delete()
                file.parentFile?.delete()
            }
        }
    }

    @Test
    fun appCoordinatorUploadsFavoritesAndEncryptedKeys() = runBlocking {
        val previous = CloudSync.config(testContext)
        org.junit.Assume.assumeFalse("Use a test install with cloud sync disabled", previous.optBoolean("enabled"))
        val oldKey = MusicRepository.apiKey(testContext)
        val oldRecords = MusicRepository.prefs(testContext).getString("cloud_records", null)
        val song = OnlineSong("chksz-qq", UUID.randomUUID().toString(), "Cloud fixture", "Artist", "Album", null)
        val key = "fixture-api-${UUID.randomUUID()}"
        WebDavFixture().use { server ->
            val account = CloudSync.sha256(server.url + "\u0000fixture-user")
            try {
                MusicRepository.setApiKey(testContext, key)
                MusicRepository.setFavorite(testContext, song, true)
                CloudSync.save(testContext, config(server.url).put("sync_keys", true)
                    .put("passphrase", "fixture-sync-passphrase"))
                val result = withTimeout(20_000) { CloudSync.state.first { !it.busy && it.message == "同步完成" } }
                assertFalse(result.failed)
                val documents = server.files.values.toList()
                assertTrue(documents.any { JSONObject(it).optJSONObject("entries")?.has("song/${song.key}/song") == true })
                assertTrue(documents.any { JSONObject(it).optJSONObject("entries")?.has("secret/music/chksz") == true })
                assertTrue(documents.none { it.contains(key) || it.contains("fixture-password") || it.contains("fixture-sync-passphrase") })
                assertEquals(key, MusicRepository.apiKey(testContext))
            } finally {
                CloudSync.save(testContext, previous)
                synchronized(CloudSync.localLock) {
                    val editor = MusicRepository.prefs(testContext).edit()
                    if (oldRecords == null) editor.remove("cloud_records") else editor.putString("cloud_records", oldRecords)
                    check(editor.commit())
                    MusicRepository.setApiKey(testContext, oldKey)
                    SecretStore.put(testContext, mapOf("sync/$account/keys" to ""))
                }
                java.io.File(testContext.filesDir, "sync/$account/${previous.getString("device_id")}.json").delete()
                java.io.File(testContext.filesDir, "sync/$account").delete()
            }
        }
    }

    /** Local-only, isolated DAV server; never accesses a user's server or library. */
    private class WebDavFixture : AutoCloseable {
        val socket = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
        val url = "http://127.0.0.1:${socket.localPort}/dav"
        val files = java.util.concurrent.ConcurrentHashMap<String, String>()
        val methods = CopyOnWriteArrayList<String>()
        val errors = CopyOnWriteArrayList<String>()
        private val worker = thread(name = "dav-fixture", isDaemon = true) {
            while (!socket.isClosed) {
                try {
                    socket.accept().use { client ->
                        client.soTimeout = 5_000
                        val reader = client.getInputStream().bufferedReader()
                        val request = reader.readLine().split(' ')
                        val headers = mutableMapOf<String, String>()
                        while (true) {
                            val line = reader.readLine() ?: break
                            if (line.isEmpty()) break
                            headers[line.substringBefore(':').lowercase()] = line.substringAfter(':').trim()
                        }
                        val length = headers["content-length"]?.toInt() ?: 0
                        val chars = CharArray(length)
                        var read = 0
                        while (read < length) {
                            val count = reader.read(chars, read, length - read)
                            check(count > 0)
                            read += count
                        }
                        val expected = "Basic " + android.util.Base64.encodeToString("fixture-user:fixture-password".toByteArray(), android.util.Base64.NO_WRAP)
                        check(headers["authorization"] == expected)
                        val method = request[0]
                        val path = request[1]
                        methods += method
                        var body = ""
                        val status = when (method) {
                            "MKCOL" -> 201
                            "PROPFIND" -> {
                                body = "<d:multistatus xmlns:d=\"DAV:\">" + files.keys.filter { it.startsWith(path) }
                                    .joinToString("") { "<d:response><d:href>$it</d:href></d:response>" } + "</d:multistatus>"
                                207
                            }
                            "PUT" -> { files[path] = String(chars); 201 }
                            "GET" -> { body = files[path] ?: ""; if (files.containsKey(path)) 200 else 404 }
                            "DELETE" -> { files.remove(path); 204 }
                            else -> 405
                        }
                        val bytes = body.toByteArray()
                        client.getOutputStream().apply {
                            write("HTTP/1.1 $status OK\r\nContent-Length: ${bytes.size}\r\nConnection: close\r\nETag: \"fixture\"\r\n\r\n".toByteArray())
                            write(bytes); flush()
                        }
                    }
                } catch (error: Exception) {
                    if (!socket.isClosed) errors += error.toString()
                }
            }
        }
        override fun close() { socket.close(); worker.join(2_000) }
    }
}
