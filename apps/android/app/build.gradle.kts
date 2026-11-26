import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.ksp)
}

// Repository root: apps/android -> apps -> <repo>
val repoRoot: File = rootProject.projectDir.parentFile.parentFile
val generatedFfiDir = layout.projectDirectory.dir("src/main/java/uniffi/openmesh_ffi").asFile
val hasGeneratedFfi = generatedFfiDir.isDirectory && generatedFfiDir.walkTopDown().any { it.extension == "kt" }

android {
    namespace = "org.openmesh.android"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.openmesh.android"
        minSdk = 26
        targetSdk = 35
        versionCode = 7
        versionName = "0.6.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        vectorDrawables { useSupportLibrary = true }
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    signingConfigs {
        val keystorePath = System.getenv("ANDROID_KEYSTORE_FILE")
        if (!keystorePath.isNullOrBlank() && File(keystorePath).isFile) {
            create("release") {
                storeFile = File(keystorePath)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release")
        }
        debug {
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets {
        getByName("main") {
            // UniFFI-generated Kotlin and the .so files are build outputs (see scripts/build-ffi.sh).
            java.srcDir("src/main/java")
            jniLibs.srcDir("src/main/jniLibs")
            // Local-only stub bindings so the app can compile before the FFI crate/bindings exist.
            // The stub is excluded as soon as real generated bindings are present. Never shipped
            // when build-ffi.sh has run (CI and release always run it first).
            if (!hasGeneratedFfi) {
                java.srcDir("src/ffiStub/java")
                logger.warn("OpenMesh: generated UniFFI bindings not found; compiling with the local stub (run scripts/build-ffi.sh for the real engine).")
            }
        }
    }

    packaging {
        resources.excludes += setOf("/META-INF/{AL2.0,LGPL2.1}", "META-INF/DEPENDENCIES")
        jniLibs.useLegacyPackaging = false
    }

    testOptions {
        unitTests.isIncludeAndroidResources = false
        unitTests.all { it.systemProperty("openmesh.repoRoot", repoRoot.absolutePath) }
    }

    lint {
        abortOnError = true
        warningsAsErrors = false
        // Translation/typography checks are noisy for an app that ships English-only copy.
        // NewApi is disabled for the UniFFI-generated bindings (they feature-gate java.lang.ref.Cleaner
        // for API 33+ while minSdk is 26); our own sources are checked normally.
        disable += setOf("MissingTranslation", "ExtraTranslation", "UnusedResources", "NewApi")
    }
}

// ---------------------------------------------------------------------------------------------
// Bundled content: the help corpus and the shared beginner presets. The web demo copies the same
// files; content/ is the single source of truth (docs/design/android-ga.md §3).
// ---------------------------------------------------------------------------------------------
val copyHelpAssets = tasks.register<Copy>("copyHelpAssets") {
    from(File(repoRoot, "content/help")) { include("*.md", "index.json") }
    into(layout.projectDirectory.dir("src/main/assets/help"))
}
val copyPresetAssets = tasks.register<Copy>("copyPresetAssets") {
    from(File(repoRoot, "content/presets")) { include("*.json") }
    into(layout.projectDirectory.dir("src/main/assets/presets"))
}
tasks.named("preBuild").configure { dependsOn(copyHelpAssets, copyPresetAssets) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.datastore.preferences)

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons.extended)
    debugImplementation(libs.compose.ui.tooling)

    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.coil.compose)

    // Room: messages, contacts, channels, node cache. No PSKs are persisted here.
    implementation(libs.room.runtime)
    implementation(libs.room.ktx)
    ksp(libs.room.compiler)

    // Map: osmdroid (MapLibre native is an alternative noted in the README).
    implementation(libs.osmdroid.android)

    // QR scanning/rendering for contact/channel invites.
    implementation(libs.zxing.android.embedded)
    implementation(libs.zxing.core)

    // Companion transports: USB CDC serial + BLE Nordic-UART-like service.
    implementation(libs.usb.serial.android)
    implementation(libs.nordic.ble)

    // UniFFI-generated Kotlin uses JNA to load libopenmesh_ffi.so.
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    androidTestImplementation(libs.androidx.test.junit)
}
