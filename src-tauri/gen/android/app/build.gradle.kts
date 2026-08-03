import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 36
    namespace = "com.cabalmesh.app"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.cabalmesh.app"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

/**
 * Locates the Kotlin half of `rustls-platform-verifier`.
 *
 * Every rustls feature of reqwest 0.13 pulls in `rustls-platform-verifier`,
 * and on Android that crate calls into the JVM. Without this component the
 * first HTTPS request panics with "Expect rustls-platform-verifier to be
 * initialized" — a crash that exists on no other platform, so a green desktop
 * and iOS build says nothing about it.
 *
 * The component ships inside the crate rather than on Maven Central, so its
 * path has to be discovered from cargo rather than written down; hardcoding a
 * registry path would break on the next `cargo update`.
 */
@Suppress("UNCHECKED_CAST")
fun rustlsPlatformVerifier(): Pair<String, String> {
    val metadata = providers.exec {
        commandLine(
            "cargo", "metadata", "--format-version", "1",
            "--filter-platform", "aarch64-linux-android",
            "--manifest-path", file("../../../Cargo.toml").absolutePath,
        )
    }.standardOutput.asText.get()

    val packages = (groovy.json.JsonSlurper().parseText(metadata) as Map<String, Any>)["packages"]
        as List<Map<String, Any>>
    val pkg = packages.first { it["name"] == "rustls-platform-verifier-android" }
    val manifest = file(pkg["manifest_path"] as String)

    // The version comes from cargo too. The bundled repository has no
    // `maven-metadata.xml`, so Gradle cannot resolve `latest.release` against
    // it — and writing the number down here would silently pin it while cargo
    // moved on.
    return File(manifest.parentFile, "maven").path to (pkg["version"] as String)
}

val rustlsVerifier = rustlsPlatformVerifier()

repositories {
    maven { url = uri(rustlsVerifier.first) }
}

dependencies {
    implementation("rustls:rustls-platform-verifier:${rustlsVerifier.second}")
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")