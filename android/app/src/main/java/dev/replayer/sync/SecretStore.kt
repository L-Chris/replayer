package dev.replayer.sync

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONObject

/** Small encrypted vault using the OS keystore; plaintext never enters preferences. */
object SecretStore {
    private const val ALIAS = "replayer.secrets.v1"
    private val aad = "replayer-secrets-v1".toByteArray()

    private fun key(create: Boolean): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        check(create) { "本机凭据密钥不可用 / Local credential key unavailable" }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
        }.generateKey()
    }

    private fun read(context: Context): JSONObject {
        val encoded = context.getSharedPreferences("secrets", Context.MODE_PRIVATE).getString("vault", null)
            ?: return JSONObject()
        val bytes = Base64.decode(encoded, Base64.NO_WRAP)
        require(bytes.size >= 28) { "Invalid encrypted credential store" }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(false), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
        cipher.updateAAD(aad)
        return JSONObject(String(cipher.doFinal(bytes, 12, bytes.size - 12), Charsets.UTF_8))
    }

    @Synchronized
    fun get(context: Context, name: String): String = read(context).optString(name)

    @Synchronized
    fun put(context: Context, values: Map<String, String>) {
        val json = read(context)
        values.forEach { (name, value) -> json.put(name, value) }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key(true))
        cipher.updateAAD(aad)
        val encrypted = cipher.iv + cipher.doFinal(json.toString().toByteArray())
        check(context.getSharedPreferences("secrets", Context.MODE_PRIVATE).edit()
            .putString("vault", Base64.encodeToString(encrypted, Base64.NO_WRAP)).commit()) {
            "无法保存本机凭据 / Cannot save local credentials"
        }
    }
}
