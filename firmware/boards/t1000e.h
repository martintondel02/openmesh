
// Seeed SenseCAP T1000-E: nRF52840 + LR1110 + Airoha GNSS + QMA6100P/temperature/humidity
// + 700 mAh LiPo, no screen, credit-card tracker.
//
// Pin sources (written from scratch, values cross-checked against the vendor references in
// /tmp/ref_boards/t1000e_variant.h and Seeed's open hardware repo):
//   * Seeed SenseCAP T1000-E variant (Arduino/nRF52): SPI 11/40/41, NSS 12, RESET 42 (P1.10),
//     DIO1 33 (P1.01), BUSY 7 (P0.07), TCXO 1.6 V, GNSS on Serial1 (RX 14 / TX 13),
//     GPS_EN 43 (P1.11), GPS_RESET 47 (P1.15), battery ADC P0.02, LED P0.24, button P0.06.
//   * Semtech LR1110 datasheet: DIO3 TCXO 1.6 V, internal RF switch.
#ifndef OPENMESH_BOARD_T1000E_H
#define OPENMESH_BOARD_T1000E_H
#include "board.h"
namespace om { namespace board {
const Board& get();
}  // namespace board
}  // namespace om
#endif
