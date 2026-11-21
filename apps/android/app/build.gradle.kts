plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "dev.veena.app"
    // Current AndroidX libraries need the 37.1+ minor SDK release.
    compileSdk {
        version = release(37) {
            minorApiLevel = 2
        }
    }

    defaultConfig {
        applicationId = "dev.veena.app"
        minSdk = 26
        targetSdk = 36
        versionCode = 2
        versionName = "0.2.0"

        // Where to look for signed brick updates. Empty means no automatic
        // updates. Set with -Pveena.updateUrl=... or VEENA_UPDATE_URL.
        fun setting(name: String, env: String) =
            providers.gradleProperty(name).orElse(providers.environmentVariable(env)).getOrElse("")
        buildConfigField("String", "UPDATE_URL", "\"${setting("veena.updateUrl", "VEENA_UPDATE_URL")}\"")
        buildConfigField("String", "UPDATE_KEY", "\"${setting("veena.updateKey", "VEENA_UPDATE_KEY")}\"")
    }

    // The release key comes from the environment (GitHub secrets in CI).
    // Without it, release builds fall back to the debug key.
    val keystore = providers.environmentVariable("VEENA_KEYSTORE").orNull
    if (keystore != null && file(keystore).exists()) {
        signingConfigs.create("release") {
            storeFile = file(keystore)
            storePassword = providers.environmentVariable("VEENA_KEYSTORE_PASSWORD").get()
            keyAlias = providers.environmentVariable("VEENA_KEY_ALIAS").get()
            keyPassword = providers.environmentVariable("VEENA_KEY_PASSWORD").get()
        }
    }

    buildTypes {
        release {
            // Shrink the Kotlin code and drop unused resources.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    // One APK per processor type: arm64 for phones, x86_64 for emulators.
    // Each only carries its own native library, so phone downloads stay small.
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "x86_64")
            isUniversalApk = false
        }
    }

    packaging {
        jniLibs.useLegacyPackaging = false
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2026.11.01")
    implementation(composeBom)
    implementation("androidx.compose.material3:material3:1.5.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.core:core-ktx:1.19.1")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.11.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.1")
    implementation("androidx.media3:media3-session:1.11.1")
    // Material You colour maths, used to colour the player from the artwork.
    implementation("com.materialkolor:material-color-utilities:5.0.1")
    // Drag to reorder the queue.
    implementation("sh.calvin.reorderable:reorderable:3.1.0")
    implementation("androidx.work:work-runtime-ktx:2.12.0")
    implementation("io.coil-kt.coil3:coil-compose:3.6.3")
    implementation("io.coil-kt.coil3:coil-network-okhttp:3.6.3")
    // The Kotlin bindings call the Rust core through JNA.
    implementation("net.java.dev.jna:jna:5.19.1@aar")
}
