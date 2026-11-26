# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile
# BOS-SCAN-CRASH-1: ML Kit (os_scan_code) registers its parts through Firebase ComponentDiscovery, which builds each
# registrar named in the manifest by reflection (no-argument constructor). R8 full mode (AGP 9) dropped those
# constructors and the ComponentRegistrar interface, so ML Kit had no components and GmsBarcodeScanning.getClient
# crashed the OS with a NullPointerException on a release build only. Keep every registrar whole.
-keep class * implements com.google.firebase.components.ComponentRegistrar { public <init>(); }
-keep interface com.google.firebase.components.ComponentRegistrar
