/* Memory layout for the Arduino UNO R4 WiFi (Renesas RA4M1: R7FA4M1AB3CFM).
 *
 * The Arduino bootloader occupies the first 16 KiB of flash and jumps to
 * our code at 0x4000. Keeping it intact lets us flash over plain USB.
 *
 * DON'T set FLASH ORIGIN to 0x00000000 — that erases the bootloader.
 */
MEMORY
{
  FLASH : ORIGIN = 0x00004000, LENGTH = 240K
  RAM   : ORIGIN = 0x20000000, LENGTH = 32K
}