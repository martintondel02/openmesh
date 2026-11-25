
// RAK WisBlock RAK4631 (nRF52840) inside the RAK solar enclosure, SX1262 radio module.
//
// Pin sources: RAK4631/RAK4630 variant in /tmp/ref_boards/rak4631_variant.h and the RAK4631
// datasheet. SX1262 needs DIO2 as the antenna switch and DIO3 for the 1.8 V TCXO; P1.07 is the
// radio power enable and must be driven high before init.
#ifndef OPENMESH_BOARD_RAK4631_SOLAR_H
#define OPENMESH_BOARD_RAK4631_SOLAR_H
#include "board.h"
namespace om { namespace board {
const Board& get();
}  // namespace board
}  // namespace om
#endif
