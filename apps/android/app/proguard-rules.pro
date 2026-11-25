# OpenMesh release rules. Keep kotlinx.serialization generated serializers and the
# companion CBOR codec (it uses reflection only for @Serializable DTOs in this app).
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.**
-keepclassmembers class **$$serializer { *; }
-keepclasseswithmembers class * {
    kotlinx.serialization.KSerializer serializer(...);
}
-keep,allowobfuscation @kotlinx.serialization.Serializable class org.openmesh.android.**
