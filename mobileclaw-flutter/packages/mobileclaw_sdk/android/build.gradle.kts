group = "com.mobileclaw.mobileclaw_sdk"
version = "1.0-SNAPSHOT"

buildscript {
    val kotlinVersion = "2.2.20"
    repositories {
        google()
        mavenCentral()
    }

    dependencies {
        classpath("com.android.tools.build:gradle:8.11.1")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:$kotlinVersion")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

plugins {
    id("com.android.library")
    id("kotlin-android")
}

// ---- Cargo NDK integration ----
// Automatically builds libmobileclaw_core.so before Android compilation.
// Requires: rustup target add aarch64-linux-android x86_64-linux-android
//           cargo install cargo-ndk
val cargoNdkDir = File(rootProject.projectDir, "../../").absolutePath
val mobileclawCorePath = File(cargoNdkDir, "mobileclaw-core").absolutePath
val jniLibsDir = File(projectDir, "src/main/jniLibs").absolutePath

val cargoNdkBuild = tasks.register<Exec>("cargoNdkBuild") {
    workingDir = File(cargoNdkDir)
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a", "-t", "x86_64",
        "-o", jniLibsDir,
        "build", "--release", "-p", "mobileclaw-core"
    )
}

// Wire into every build variant (debug, release, profile)
android.libraryVariants.all {
    preBuildProvider.configure { dependsOn(cargoNdkBuild) }
}

android {
    namespace = "com.mobileclaw.mobileclaw_sdk"

    compileSdk = 36

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = JavaVersion.VERSION_17.toString()
    }

    sourceSets {
        getByName("main") {
            java.srcDirs("src/main/kotlin")
            // Pre-built Rust native libraries for FFI bridge.
            jniLibs.srcDirs("src/main/jniLibs")
        }
        getByName("test") {
            java.srcDirs("src/test/kotlin")
        }
    }

    defaultConfig {
        minSdk = 24
    }

    testOptions {
        unitTests {
            isIncludeAndroidResources = true
            all {
                it.useJUnitPlatform()

                it.outputs.upToDateWhen { false }

                it.testLogging {
                    events("passed", "skipped", "failed", "standardOut", "standardError")
                    showStandardStreams = true
                }
            }
        }
    }
}

dependencies {
    testImplementation("org.jetbrains.kotlin:kotlin-test")
    testImplementation("org.mockito:mockito-core:5.0.0")
}
