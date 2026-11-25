// SPDX-License-Identifier: AGPL-3.0-or-later
// Compile-time board selection. Exactly one -DOM_BOARD_* macro must be set by platformio.ini.
#ifndef OPENMESH_BOARD_SELECT_H
#define OPENMESH_BOARD_SELECT_H

#if defined(OM_BOARD_T1000E)
#include "t1000e.h"
#elif defined(OM_BOARD_RAK4631_SOLAR)
#include "rak4631_solar.h"
#elif defined(OM_BOARD_SEEED_SOLAR)
#include "seeed_solar.h"
#elif defined(OM_BOARD_HELTEC_TRACKER)
#include "heltec_tracker.h"
#else
#error "No board selected: define one of OM_BOARD_T1000E, OM_BOARD_RAK4631_SOLAR, OM_BOARD_SEEED_SOLAR, OM_BOARD_HELTEC_TRACKER"
#endif

#endif  // OPENMESH_BOARD_SELECT_H
