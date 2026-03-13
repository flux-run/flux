// Build: ./gradlew wasmWasiBinaries
// Install: Kotlin >= 1.9 — https://kotlinlang.org/docs/wasm-get-started.html
plugins {
    kotlin("multiplatform") version "2.0.0"
}

kotlin {
    wasmWasi {
        binaries.executable()
    }

    sourceSets {
        val wasmWasiMain by getting
    }
}
