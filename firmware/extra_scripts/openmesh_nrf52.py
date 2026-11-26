# SPDX-License-Identifier: AGPL-3.0-or-later
"""Extra include paths for the nRF52 Arduino framework.

The Meshtastic nRF52 platform fork uses the git version of the Adafruit nRF52 Arduino
framework. Two directories that the Adafruit BSP package normally ships are absent from the
plain git checkout:

* ``cores/nRF5/cmsis`` — supplied here by the PlatformIO ``framework-cmsis`` tool package
  (declared in each nRF52 ``platform_packages``).
* ``cores/nRF5/TinyUSB`` — the platform builder expects the TinyUSB headers there; in the git
  tree they live inside ``libraries/Adafruit_TinyUSB_Arduino/src``.

Adding those include paths here keeps the board builds reproducible from a plain
``pio run -e t1000e`` without patching PlatformIO's package cache.
"""
from glob import glob
from os.path import isdir, join

Import("env")

packages = env["PROJECT_PACKAGES_DIR"]

# CMSIS Core headers (core_cm4.h and friends), from the framework-cmsis tool package.
for package_dir in sorted(glob(join(packages, "framework-cmsis*"))):
    include = join(package_dir, "CMSIS", "Core", "Include")
    if isdir(include):
        env.Append(CPPPATH=[include])
        break

# TinyUSB Arduino library shipped inside the Adafruit nRF52 framework (needed by Arduino.h
# when USE_TINYUSB is defined).
for package_dir in sorted(glob(join(packages, "framework-arduinoadafruitnrf52*"))):
    src = join(package_dir, "libraries", "Adafruit_TinyUSB_Arduino", "src")
    if isdir(src):
        env.Append(CPPPATH=[src, join(src, "arduino"), join(src, "class")])
        break

# Adafruit's FreeRTOS heap_3 requires the wrap linker flags that the BSP platform.txt has but
# the PlatformIO nRF52 builder omits. Without them the link fails with __real_malloc/free.
platform = env.get("PIOPLATFORM", "")
if "nordicnrf52" in platform:
    env.Append(LINKFLAGS=[
        "-Wl,--wrap=malloc",
        "-Wl,--wrap=free",
        "-Wl,--wrap=realloc",
        "-Wl,--wrap=calloc",
    ])
