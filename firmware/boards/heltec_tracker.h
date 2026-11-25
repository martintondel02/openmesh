
// Heltec Wireless Tracker V1.1: ESP32-S3 + SX1262 + L76K/UC6580 GNSS + 0.96" ST7735S TFT.
//
// Pin sources: /tmp/ref_boards/hwt_variant.h and the Heltec Wireless Tracker schematic.
// VEXT (GPIO3) powers the GNSS, its LNA and the display; keep it high while GPS/TFT are used.
#ifndef OPENMESH_BOARD_HELTEC_TRACKER_H
#define OPENMESH_BOARD_HELTEC_TRACKER_H
#include "board.h"
namespace om { namespace board {
const Board& get();
}  // namespace board
}  // namespace om
#endif
