// OpenMesh Android client — Gradle settings.
//
// Build: ./gradlew assembleDebug
// The Gradle wrapper jar is checked in so CI can build without a system Gradle.

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        // usb-serial-for-android is published on JitPack.
        maven { url = uri("https://jitpack.io") }
    }
}

rootProject.name = "OpenMesh"
include(":app")
