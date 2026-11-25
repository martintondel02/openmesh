
// Seeed SenseCAP Solar Node P1: nRF52840 (XIAO nRF52840 class board) + SX1262 + L76KB GNSS,
// solar/battery pack, IPX antenna, no screen.
//
// Pin sources: /tmp/ref_boards/solar_variant.h (D0-D19 map and SX126X_* definitions) plus the
// Seeed SenseCAP Solar Node P1 documentation. SX1262 uses an external RX switch on D5 and
// DIO2 as the RF switch; DIO3 supplies the 1.8 V TCXO.
#ifndef OPENMESH_BOARD_SEEED_SOLAR_H
#define OPENMESH_BOARD_SEEED_SOLAR_H
#include "board.h"
namespace om { namespace board {
const Board& get();
}  // namespace board
}  // namespace om
#endif
