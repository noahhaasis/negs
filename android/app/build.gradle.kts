plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

// The engine's data (stock profiles, LUTs, borders…), as the desktop apps
// ship it, less what only builds profiles. Unpacked on first launch
// (EngineData.kt): the engine reads files.
val engineData = layout.buildDirectory.dir("generated/engineData")
val syncEngineData by tasks.registering(Sync::class) {
    from(rootProject.file("../data")) {
        exclude("profile-sources/**")
    }
    into(engineData.map { it.dir("data") })
}
// The web app's sample photograph, for "Try a sample".
val syncSamples by tasks.registering(Sync::class) {
    from(rootProject.file("../web/sample")) { include("DSC_6377.dng") }
    into(layout.buildDirectory.dir("generated/samples/samples"))
}
// The web app's translations (I18n.kt): the app's own strings are in
// src/main/assets/locales.
val syncLocales by tasks.registering(Sync::class) {
    from(rootProject.file("../web/locales")) { include("*.js") }
    into(layout.buildDirectory.dir("generated/locales/locales"))
}
// Optional 60 MP Leica Q3 RAW for Firebase Test Lab instrumentation benchmarks.
val largeRawPath = providers.gradleProperty("negsLargeRaw").orNull
val syncLargeRaw by tasks.registering(Sync::class) {
    if (largeRawPath != null) {
        from(file(largeRawPath)) { rename { "leica60mp.dng" } }
    }
    into(layout.buildDirectory.dir("generated/testSamples/samples"))
}

android {
    namespace = "dev.negs.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.negs.app"
        // Vulkan 1.1 is required from Android 10.
        minSdk = 29
        targetSdk = 35
        versionCode = 1
        versionName = "0.20.1"
        ndk { abiFilters += "arm64-v8a" }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
    sourceSets["main"].assets.srcDir(engineData)
    sourceSets["main"].assets.srcDir(layout.buildDirectory.dir("generated/samples"))
    sourceSets["main"].assets.srcDir(layout.buildDirectory.dir("generated/locales"))
    sourceSets["androidTest"].assets.srcDir(layout.buildDirectory.dir("generated/testSamples"))
    packaging {
        // The engine runs as a program from the native library directory,
        // so it must be unpacked there, not mapped from the APK.
        jniLibs.useLegacyPackaging = true
    }
    androidResources {
        // PNGs (the borders, most of the data) and DNGs are compressed already.
        noCompress += listOf("png", "dng")
    }
}

tasks.named("preBuild") { dependsOn(syncEngineData, syncSamples, syncLocales, syncLargeRaw) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-process:2.8.7")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:rules:1.6.1")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
}
