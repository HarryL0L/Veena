plugins {
    // AGP 9 compiles Kotlin itself; only the Compose compiler plugin is added.
    id("com.android.application") version "9.5.2" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.4.21" apply false
}
