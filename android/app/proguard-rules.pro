# remdia ships as a local AAR file dependency, whose consumer proguard.txt is
# not applied by AGP, so its JNI-referenced classes are kept here. The replayer
# JNI surface is resolved by symbol name from Kotlin `external` declarations.
-keep class dev.remdia.** { *; }
-keep class dev.replayer.Native { *; }

# rustls-platform-verifier loads its JVM companion by name through the app class
# loader and resolves members with GetMethodID/GetFieldID, so R8 must neither
# rename nor strip it.
-keep class org.rustls.platformverifier.** { *; }
