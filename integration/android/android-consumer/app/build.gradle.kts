plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val kurmanciGroup = project.findProperty("kurmanciMavenGroup")?.toString() ?: "io.github.ferhatguneri"
val kurmanciVersion = project.findProperty("kurmanciVersion")?.toString() ?: "0.1.0"

android {
    namespace = "org.kurmanci.consumer"
    compileSdk = 34

    defaultConfig {
        applicationId = "org.kurmanci.consumer"
        minSdk = 23
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
}

dependencies {
    implementation("$kurmanciGroup:kurmanci-android:$kurmanciVersion")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.11.1")

    androidTestImplementation("androidx.test.ext:junit:1.1.5")
    androidTestImplementation("androidx.test:runner:1.5.2")
    androidTestImplementation("androidx.test:rules:1.5.0")
}
