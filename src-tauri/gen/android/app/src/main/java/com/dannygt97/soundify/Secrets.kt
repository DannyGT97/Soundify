package com.dannygt97.soundify

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Seals the small secrets Soundify keeps (the tokens it holds to be let into the user's other devices) with a key that lives in
 * the Android Keystore and never leaves it: what is written to disk cannot be opened by another app, nor from a copy of the
 * files. AES-256-GCM; what comes back is the 12 bytes of the iv followed by the sealed bytes.
 */
object Secrets {
  private const val ALIAS = "soundify-secrets"
  private const val IV_BYTES = 12
  private const val TAG_BITS = 128

  private fun key(): SecretKey {
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generator.init(
      KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build(),
    )
    return generator.generateKey()
  }

  fun seal(plain: ByteArray): ByteArray {
    val cipher = Cipher.getInstance("AES/GCM/NoPadding")
    cipher.init(Cipher.ENCRYPT_MODE, key())
    return cipher.iv + cipher.doFinal(plain)
  }

  fun open(sealed: ByteArray): ByteArray {
    require(sealed.size > IV_BYTES) { "too short to be sealed" }
    val cipher = Cipher.getInstance("AES/GCM/NoPadding")
    cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, sealed.copyOfRange(0, IV_BYTES)))
    return cipher.doFinal(sealed, IV_BYTES, sealed.size - IV_BYTES)
  }
}
