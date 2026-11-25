package org.openmesh.android.data.security

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import org.openmesh.android.data.prefs.AppPreferences
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Owns the 32-byte identity seed. The seed is generated on the device and never leaves it in
 * plaintext storage: it is wrapped with an AES-GCM key held by AndroidKeyStore and only the
 * ciphertext (IV || cipher) is persisted in DataStore. Losing the Keystore (factory reset, key
 * invalidation) loses the identity, which matches the "no account recovery" rule.
 */
class IdentityStore(context: Context) {
    private val appContext = context.applicationContext

    suspend fun loadOrCreateSeed(prefs: AppPreferences): ByteArray {
        val stored = prefs.current().identityCiphertext
        if (!stored.isNullOrBlank()) {
            return decrypt(stored)
                ?: throw IdentityException("The identity key could not be unwrapped; restore a backup or reset the identity.")
        }
        val seed = ByteArray(32).also { java.security.SecureRandom().nextBytes(it) }
        prefs.setIdentityCiphertext(encrypt(seed))
        return seed
    }

    suspend fun clear(prefs: AppPreferences) {
        prefs.setIdentityCiphertext("")
    }

    /** Exports the wrapped identity for an explicit backup (still requires the device Keystore). */
    suspend fun exportWrapped(prefs: AppPreferences): String? = prefs.current().identityCiphertext

    private fun encrypt(seed: ByteArray): String {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val iv = cipher.iv
        val encrypted = cipher.doFinal(seed)
        val combined = ByteArray(iv.size + encrypted.size)
        System.arraycopy(iv, 0, combined, 0, iv.size)
        System.arraycopy(encrypted, 0, combined, iv.size, encrypted.size)
        return Base64.encodeToString(combined, Base64.NO_WRAP)
    }

    private fun decrypt(wrapped: String): ByteArray? = runCatching {
        val combined = Base64.decode(wrapped, Base64.NO_WRAP)
        require(combined.size > IV_BYTES)
        val iv = combined.copyOfRange(0, IV_BYTES)
        val data = combined.copyOfRange(IV_BYTES, combined.size)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, iv))
        cipher.doFinal(data)
    }.getOrNull()

    private fun key(): SecretKey {
        val store = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (store.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    companion object {
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private const val KEY_ALIAS = "openmesh.identity.v1"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val IV_BYTES = 12
        private const val TAG_BITS = 128
    }
}

class IdentityException(message: String) : Exception(message)
