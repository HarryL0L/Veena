# The Kotlin bindings reach the Rust core through JNA, which finds classes,
# fields and callbacks by reflection: keep them as they are.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.**

# The token maker's web view calls these by name.
-keepclassmembers class * {
    @android.webkit.JavascriptInterface <methods>;
}

# Annotation-only classes referenced by libraries but not needed at run time.
-dontwarn com.google.errorprone.annotations.**
-dontwarn com.google.j2objc.annotations.**
-dontwarn javax.annotation.**
-dontwarn org.checkerframework.**
