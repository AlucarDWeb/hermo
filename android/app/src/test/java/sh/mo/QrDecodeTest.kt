package sh.mo

import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.HybridBinarizer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the decoder half of the no-GMS scanner: the payload printed by
 * `scripts/hermes-qr.sh` must survive a full QR decode through the same
 * ZXing `MultiFormatReader` the scan screen uses. The QR is rebuilt from
 * the PNG's black/white module grid (33x33, qrencode v4-M; extracted from
 * the PNG the script wrote) — no camera, no Android, pure JVM. If the
 * script's encoding breaks, the decode returns a different string and this
 * fails.
 */
class QrDecodeTest {

    companion object {
        // '1' = black module. Extracted from the PNG rendered by
        // scripts/hermes-qr.sh --png for the live gateway payload.
        private val GRID_ROWS: List<String> = listOf(
            "111111100000110001111111001111111",
            "100000101111011000011110101000001",
            "101110100100000111110100001011101",
            "101110101001101000000010001011101",
            "101110100001110100111111001011101",
            "100000101111001111001100101000001",
            "111111101010101010101010101111111",
            "000000000010011110111100100000000",
            "111110111110110001101001110101010",
            "101010000101010001010101011001101",
            "001110111101001010110100010011110",
            "100101001100011010100000000101110",
            "000000111101111100010011000111011",
            "000111000001100111101101111000101",
            "001001111001001100110010111001010",
            "111111011110010011100000011000101",
            "011101111011101000111001110111000",
            "110000000010010001001111011000011",
            "100100110010101000111100110110010",
            "101000001101011110011111101010110",
            "111111101011101001010000000110010",
            "110100010111110010110011011001001",
            "100111111111101100101110011111010",
            "101000000010000011110110010010100",
            "101011110011110001100100111110001",
            "000000001010110001001101100011001",
            "111111101000001001100101101011010",
            "100000100010011011110011100011110",
            "101110101001110000110000111110011",
            "101110101101110110110100000110110",
            "101110101111001001100011111001000",
            "100000101111010110000110000011100",
            "111111101100110100011110110010010",
        )
    }

    @Test
    fun `script payload survives a full zxing decode`() {
        val rows = GRID_ROWS
        val n = rows.size
        assertTrue("expected 33 rows", n == 33)
        assertTrue("grid must be square", rows.all { it.length == n })

        // Build an RGB image from the module grid (upscaled x4 so the
        // binarizer has room, mirroring a real camera frame's proportions).
        val scale = 4
        val size = n * scale
        val pixels = IntArray(size * size)
        for (y in 0 until size) for (x in 0 until size) {
            val black = rows[y / scale][x / scale] == '1'
            val v = if (black) 0 else 0xFF
            pixels[y * size + x] = (v shl 16) or (v shl 8) or v
        }
        val source = RGBLuminanceSource(size, size, pixels)
        val reader = MultiFormatReader().apply {
            setHints(mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE)))
        }
        val text = reader.decode(BinaryBitmap(HybridBinarizer(source)))

        assertEquals(
            "hermes://connect?v=1&url=http%3A%2F%2F192.168.1.48%3A9123&user=hermo&name=hermo-lan",
            text.text,
        )
    }
}
